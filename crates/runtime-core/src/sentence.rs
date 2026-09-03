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
    buffer_source_start: usize,
    next_sentence_id: u64,
}

/// A complete, sanitized sentence and its exact UTF-8 byte range in the original
/// provider stream. The range is the authority used by delivered-only memory
/// commits; callers never have to infer offsets from trimmed text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SentenceSpan {
    pub sentence_id: u64,
    pub text_start_bytes: usize,
    pub text_end_bytes: usize,
    pub text: String,
}

impl SentenceSegmenter {
    pub fn new(config: SentenceSegmenterConfig) -> Self {
        assert!(config.min_sentence_chars > 0);
        assert!(config.soft_boundary_chars >= config.min_sentence_chars);
        assert!(config.hard_limit_chars >= config.soft_boundary_chars);
        Self {
            config,
            buffer: String::new(),
            buffer_source_start: 0,
            next_sentence_id: 1,
        }
    }

    pub fn push(&mut self, delta: &str) -> Vec<String> {
        self.push_spans(delta)
            .into_iter()
            .map(|span| span.text)
            .collect()
    }

    pub fn push_spans(&mut self, delta: &str) -> Vec<SentenceSpan> {
        self.buffer.push_str(delta);
        self.extract(false)
    }

    pub fn finish(&mut self) -> Vec<String> {
        self.finish_spans()
            .into_iter()
            .map(|span| span.text)
            .collect()
    }

    pub fn finish_spans(&mut self) -> Vec<SentenceSpan> {
        self.extract(true)
    }

    pub fn pending_text(&self) -> &str {
        &self.buffer
    }

    fn extract(&mut self, flush: bool) -> Vec<SentenceSpan> {
        let mut emitted = Vec::new();
        while let Some(boundary) = self.find_boundary(flush) {
            let remainder = self.buffer.split_off(boundary);
            let raw_sentence = std::mem::replace(&mut self.buffer, remainder);
            let leading = raw_sentence.len() - raw_sentence.trim_start().len();
            let trimmed_end = raw_sentence.trim_end().len();
            let sentence = raw_sentence[leading..trimmed_end].to_owned();
            if !sentence.is_empty() {
                emitted.push(SentenceSpan {
                    sentence_id: self.next_sentence_id,
                    text_start_bytes: self.buffer_source_start + leading,
                    text_end_bytes: self.buffer_source_start + trimmed_end,
                    text: sentence,
                });
                self.next_sentence_id += 1;
            }
            self.buffer_source_start += boundary;
            if self.buffer.trim().is_empty() {
                self.buffer_source_start += self.buffer.len();
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
        // The buffer is deliberately rescanned. It is capped by hard_limit_chars,
        // and rescanning is required when a punctuation mark arrives in one delta
        // while its following whitespace arrives in the next delta.
        for (byte, ch) in self.buffer.char_indices() {
            let end = byte + ch.len_utf8();
            let char_count = self.buffer[..end].chars().count();

            if matches!(ch, ',' | ';' | ':' | '\u{2014}') {
                last_soft = Some(end);
            }

            if matches!(ch, '!' | '?' | '\n') && char_count >= self.config.min_sentence_chars {
                if ch == '\n' {
                    return Some(self.consume_whitespace(end));
                }
                if let Some(boundary) = self.terminal_boundary(end) {
                    return Some(boundary);
                }
            }

            if ch == '.'
                && char_count >= self.config.min_sentence_chars
                && !self.is_decimal(byte, previous)
                && !self.is_abbreviation(end)
            {
                if let Some(boundary) = self.terminal_boundary(end) {
                    return Some(boundary);
                }
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
        None
    }

    fn terminal_boundary(&self, mut end: usize) -> Option<usize> {
        for ch in self.buffer[end..].chars() {
            if matches!(ch, '\"' | '\'' | ')' | ']' | '}') {
                end += ch.len_utf8();
                continue;
            }
            return ch.is_whitespace().then(|| self.consume_whitespace(end));
        }
        // End-of-buffer punctuation is held until either finish() or the next
        // chunk proves the boundary. This avoids splitting abbreviations and
        // keeps closing quotes attached to the spoken sentence.
        None
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
    use proptest::prelude::*;

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
    fn reconsiders_terminal_punctuation_when_whitespace_arrives_later() {
        let mut segmenter = SentenceSegmenter::default();
        assert!(segmenter
            .push("This sentence ends exactly at a chunk boundary.")
            .is_empty());
        assert_eq!(
            segmenter.push(" Next sentence"),
            vec!["This sentence ends exactly at a chunk boundary."]
        );
        assert_eq!(segmenter.finish(), vec!["Next sentence"]);
    }

    #[test]
    fn spans_preserve_utf8_offsets_and_closing_quotes() {
        let mut segmenter = SentenceSegmenter::default();
        let source = "  She said, \"Meet me by the 界 gate!\"  Then she left.";
        let mut spans = segmenter.push_spans(source);
        spans.extend(segmenter.finish_spans());
        assert_eq!(spans.len(), 2);
        for span in &spans {
            assert_eq!(
                &source[span.text_start_bytes..span.text_end_bytes],
                span.text
            );
        }
        assert_eq!(spans[0].text, "She said, \"Meet me by the 界 gate!\"");
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

    proptest! {
        #[test]
        fn segmentation_is_independent_of_provider_chunking(
            words in prop::collection::vec("[A-Za-z]{1,10}", 4..60)
        ) {
            let mut source = String::new();
            for (index, word) in words.iter().enumerate() {
                source.push_str(word);
                if index % 5 == 4 {
                    source.push_str(". ");
                } else {
                    source.push(' ');
                }
            }

            let mut one_chunk = SentenceSegmenter::default();
            let mut expected = one_chunk.push(&source);
            expected.extend(one_chunk.finish());

            let mut character_chunks = SentenceSegmenter::default();
            let mut actual = Vec::new();
            for character in source.chars() {
                actual.extend(character_chunks.push(&character.to_string()));
            }
            actual.extend(character_chunks.finish());
            prop_assert_eq!(actual, expected);
        }
    }
}
