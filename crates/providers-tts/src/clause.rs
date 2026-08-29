/// Semantic chunking policy shared across hosted providers. It holds incomplete
/// tokens rather than forwarding arbitrary LLM network chunks to speech.
#[derive(Clone, Debug)]
pub struct SemanticClausePolicy {
    pub minimum_chars: usize,
    pub preferred_chars: usize,
    pub maximum_chars: usize,
}

impl Default for SemanticClausePolicy {
    fn default() -> Self {
        Self {
            minimum_chars: 18,
            preferred_chars: 96,
            maximum_chars: 220,
        }
    }
}

impl SemanticClausePolicy {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.minimum_chars == 0
            || self.preferred_chars < self.minimum_chars
            || self.maximum_chars < self.preferred_chars
            || self.maximum_chars > 2_000
        {
            return Err("invalid_clause_policy");
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SemanticClauseBuffer {
    policy: SemanticClausePolicy,
    pending: String,
}

impl SemanticClauseBuffer {
    pub(crate) fn new(policy: SemanticClausePolicy) -> Self {
        Self {
            policy,
            pending: String::new(),
        }
    }

    pub(crate) fn push(&mut self, text: &str) -> Vec<String> {
        self.pending.push_str(text);
        self.extract(false)
    }

    pub(crate) fn finish(&mut self) -> Vec<String> {
        self.extract(true)
    }

    pub(crate) fn pending_chars(&self) -> usize {
        self.pending.chars().count()
    }

    fn extract(&mut self, flush: bool) -> Vec<String> {
        let mut output = Vec::new();
        while let Some(boundary) = self.next_boundary(flush) {
            let remainder = self.pending.split_off(boundary);
            let clause = std::mem::replace(&mut self.pending, remainder);
            let clause = clause.trim().to_owned();
            if !clause.is_empty() {
                output.push(clause);
            }
            if self.pending.trim().is_empty() {
                self.pending.clear();
                break;
            }
        }
        output
    }

    fn next_boundary(&self, flush: bool) -> Option<usize> {
        if flush {
            return (!self.pending.trim().is_empty()).then_some(self.pending.len());
        }

        let mut character_count = 0;
        let mut preferred_boundary = None;
        for (byte, character) in self.pending.char_indices() {
            character_count += 1;
            let end = byte + character.len_utf8();
            let next = self.pending[end..].chars().next();
            let token_boundary = next.is_none_or(char::is_whitespace);

            if character_count >= self.policy.minimum_chars
                && token_boundary
                && matches!(character, '.' | '!' | '?' | '\n')
                && !self.looks_like_abbreviation(end)
                && !self.looks_like_decimal(byte)
            {
                return Some(self.consume_whitespace(end));
            }

            if character_count >= self.policy.minimum_chars
                && token_boundary
                && matches!(character, ',' | ';' | ':' | '\u{2014}')
            {
                preferred_boundary = Some(end);
            }

            if character_count >= self.policy.preferred_chars {
                if let Some(boundary) = preferred_boundary {
                    return Some(self.consume_whitespace(boundary));
                }
            }
            if character_count >= self.policy.maximum_chars {
                return Some(self.word_boundary_at_or_before(end));
            }
        }
        None
    }

    fn consume_whitespace(&self, mut end: usize) -> usize {
        for character in self.pending[end..].chars() {
            if character.is_whitespace() {
                end += character.len_utf8();
            } else {
                break;
            }
        }
        end
    }

    fn word_boundary_at_or_before(&self, end: usize) -> usize {
        self.pending[..end]
            .char_indices()
            .rev()
            .find(|(_, character)| character.is_whitespace())
            .map_or(end, |(byte, character)| byte + character.len_utf8())
    }

    fn looks_like_decimal(&self, period: usize) -> bool {
        let before = self.pending[..period].chars().next_back();
        let after = self.pending[period + 1..].chars().next();
        before.is_some_and(|character| character.is_ascii_digit())
            && after.is_some_and(|character| character.is_ascii_digit())
    }

    fn looks_like_abbreviation(&self, end: usize) -> bool {
        let token = self.pending[..end]
            .split_whitespace()
            .next_back()
            .unwrap_or_default()
            .trim_matches(|character: char| matches!(character, '"' | '\'' | '(' | '['))
            .to_ascii_lowercase();
        matches!(
            token.as_str(),
            "mr." | "mrs." | "ms." | "dr." | "prof." | "sr." | "jr." | "vs." | "e.g." | "i.e."
        ) || (token.len() <= 3
            && token.ends_with('.')
            && token[..token.len() - 1]
                .chars()
                .all(|character| character.is_ascii_alphabetic()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_complete_semantic_clauses_across_arbitrary_chunks() {
        let mut buffer = SemanticClauseBuffer::new(SemanticClausePolicy::default());
        assert!(buffer.push("The reactor is sta").is_empty());
        assert_eq!(
            buffer.push("ble, captain. We can safely move now! Another line"),
            vec!["The reactor is stable, captain.", "We can safely move now!"]
        );
        assert_eq!(buffer.finish(), vec!["Another line"]);
    }

    #[test]
    fn does_not_split_abbreviations_decimals_or_utf8() {
        let mut buffer = SemanticClauseBuffer::new(SemanticClausePolicy {
            minimum_chars: 4,
            preferred_chars: 10,
            maximum_chars: 14,
        });
        let output = buffer.push("Dr. V paid 3.14 credits. 世界 世界 世界 世界");
        assert_eq!(output[0], "Dr. V paid");
        assert!(output
            .iter()
            .all(|clause| clause.is_char_boundary(clause.len())));
    }
}
