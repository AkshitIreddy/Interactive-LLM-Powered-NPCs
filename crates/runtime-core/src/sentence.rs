use std::collections::HashSet;

/// Streaming sentence segmentation tuned for low-latency TTS clause dispatch.
#[derive(Clone, Debug)]
pub struct SentenceSegmenterConfig {
    pub min_sentence_chars: usize,
    pub soft_boundary_chars: usize,
    pub hard_limit_chars: usize,
    pub abbreviations: HashSet<String>,
}

impl Default for SentenceSegmenterConfig {
    fn default() -> Self {
        Self {
            min_sentence_chars: 18,
            soft_boundary_chars: 96,
            hard_limit_chars: 220,
            abbreviations: [
                "mr.", "mrs.", "ms.", "dr.", "prof.", "sr.", "jr.", "st.", "vs.", "e.g.", "i.e.",
                "etc.", "u.s.",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct SentenceSegmenter {
    config: SentenceSegmenterConfig,
    buffer: String,
    scan_from: usize,
}

impl SentenceSegmenter {
    pub fn new(config: SentenceSegmenterConfig) -> Self {
        assert!(config.min_sentence_chars > 0);
        assert!(config.soft_boundary_chars >= config.min_sentence_chars);
        assert!(config.hard_limit_chars >= config.soft_boundary_chars);
        Self {
            config,
            buffer: String::new(),
            scan_from: 0,
        }
    }

    pub fn push(&mut self, delta: &str) -> Vec<String> {
        self.buffer.push_str(delta);
        self.extract(false)
    }

    pub fn finish(&mut self) -> Vec<String> {
        self.extract(true)
    }

    pub fn pending_text(&self) -> &str {
        &self.buffer
    }

    fn extract(&mut self, flush: bool) -> Vec<String> {
        let mut emitted = Vec::new();
        while let Some(boundary) = self.find_boundary(flush) {
            let remainder = self.buffer.split_off(boundary);
            let sentence = std::mem::replace(&mut self.buffer, remainder);
            let sentence = sentence.trim().to_owned();
            self.scan_from = 0;
            if !sentence.is_empty() {
                emitted.push(sentence);
            }
            if self.buffer.trim().is_empty() {
                self.buffer.clear();
                break;
            }
        }
        emitted
    }

    fn find_boundary(&mut self, flush: bool) -> Option<usize> {
        if flush {
            return (!self.buffer.trim().is_empty()).then_some(self.buffer.len());
        }

        let mut last_soft = None;
        let mut previous = None;
        for (offset, ch) in self.buffer[self.scan_from..].char_indices() {
            let byte = self.scan_from + offset;
            let end = byte + ch.len_utf8();
            let char_count = self.buffer[..end].chars().count();

            if matches!(ch, ',' | ';' | ':' | '\u{2014}') {
                last_soft = Some(end);
            }

            if matches!(ch, '!' | '?' | '\n')
                && char_count >= self.config.min_sentence_chars
                && self.followed_by_boundary(end)
            {
                return Some(self.consume_whitespace(end));
            }

            if ch == '.'
                && char_count >= self.config.min_sentence_chars
                && !self.is_decimal(byte, previous)
                && !self.is_abbreviation(end)
                && self.followed_by_boundary(end)
            {
                return Some(self.consume_whitespace(end));
            }

            if char_count >= self.config.soft_boundary_chars {
                if let Some(soft) = last_soft {
                    return Some(self.consume_whitespace(soft));
                }
            }
            if char_count >= self.config.hard_limit_chars {
                return Some(self.best_word_boundary(end));
            }
            previous = Some(ch);
        }
        self.scan_from = self.buffer.len();
        None
    }

    fn followed_by_boundary(&self, end: usize) -> bool {
        self.buffer[end..]
            .chars()
            .next()
            .map(|next| next.is_whitespace() || matches!(next, '\"' | '\'' | ')' | ']'))
            .unwrap_or(false)
    }

    fn consume_whitespace(&self, mut end: usize) -> usize {
        for ch in self.buffer[end..].chars() {
            if ch.is_whitespace() {
                end += ch.len_utf8();
            } else {
                break;
            }
        }
        end
    }

    fn best_word_boundary(&self, end: usize) -> usize {
        self.buffer[..end]
            .char_indices()
            .rev()
            .find(|(_, ch)| ch.is_whitespace())
            .map(|(byte, ch)| byte + ch.len_utf8())
            .unwrap_or(end)
    }

    fn is_decimal(&self, period_byte: usize, previous: Option<char>) -> bool {
        previous.is_some_and(|ch| ch.is_ascii_digit())
            && self.buffer[period_byte + 1..]
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_digit())
    }

    fn is_abbreviation(&self, end: usize) -> bool {
        let prefix = &self.buffer[..end];
        let token = prefix
            .rsplit_once(char::is_whitespace)
            .map(|(_, tail)| tail)
            .unwrap_or(prefix)
            .trim_matches(|ch: char| matches!(ch, '\"' | '\'' | '(' | '['))
            .to_ascii_lowercase();
        self.config.abbreviations.contains(&token)
            || (token.len() <= 3
                && token.ends_with('.')
                && token[..token.len() - 1]
                    .chars()
                    .all(|ch| ch.is_ascii_alphabetic()))
    }
}

impl Default for SentenceSegmenter {
    fn default() -> Self {
        Self::new(SentenceSegmenterConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_across_stream_chunks() {
        let mut segmenter = SentenceSegmenter::default();
        assert!(segmenter.push("This is a complete sent").is_empty());
        assert_eq!(
            segmenter.push("ence. Here is another one!"),
            vec!["This is a complete sentence."]
        );
        assert_eq!(segmenter.finish(), vec!["Here is another one!"]);
    }

    #[test]
    fn does_not_split_decimal_or_abbreviation() {
        let mut segmenter = SentenceSegmenter::default();
        let result = segmenter.push("Dr. V paid 3.14 credits. That was surprisingly precise.");
        assert_eq!(result, vec!["Dr. V paid 3.14 credits."]);
        assert_eq!(segmenter.finish(), vec!["That was surprisingly precise."]);
    }

    #[test]
    fn unicode_hard_limit_never_slices_mid_character() {
        let config = SentenceSegmenterConfig {
            min_sentence_chars: 4,
            soft_boundary_chars: 8,
            hard_limit_chars: 12,
            ..Default::default()
        };
        let mut segmenter = SentenceSegmenter::new(config);
        let output = segmenter.push("界界界界 界界界界 界界界界");
        assert!(!output.is_empty());
        assert!(output
            .iter()
            .all(|value| value.is_char_boundary(value.len())));
    }
}
