use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VdfValue {
    Text(String),
    Object(BTreeMap<String, VdfValue>),
}

impl VdfValue {
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(value) => Some(value),
            Self::Object(_) => None,
        }
    }

    pub fn as_object(&self) -> Option<&BTreeMap<String, VdfValue>> {
        match self {
            Self::Object(value) => Some(value),
            Self::Text(_) => None,
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum VdfError {
    #[error("unexpected end of VDF input")]
    UnexpectedEnd,
    #[error("unexpected token at byte {0}")]
    UnexpectedToken(usize),
    #[error("unterminated quoted string at byte {0}")]
    UnterminatedString(usize),
    #[error("unclosed object")]
    UnclosedObject,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Text(String),
    Open,
    Close,
}

pub fn parse(input: &str) -> Result<BTreeMap<String, VdfValue>, VdfError> {
    let tokens = tokenize(input)?;
    let mut cursor = 0;
    parse_object(&tokens, &mut cursor, false)
}

fn tokenize(input: &str) -> Result<Vec<Token>, VdfError> {
    let bytes = input.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;

    while index < bytes.len() {
        match bytes[index] {
            b' ' | b'\t' | b'\r' | b'\n' => index += 1,
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index += 2;
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'{' => {
                tokens.push(Token::Open);
                index += 1;
            }
            b'}' => {
                tokens.push(Token::Close);
                index += 1;
            }
            b'"' => {
                let start = index;
                index += 1;
                let mut value = String::new();
                let mut closed = false;
                while index < bytes.len() {
                    match bytes[index] {
                        b'"' => {
                            index += 1;
                            closed = true;
                            break;
                        }
                        b'\\' => {
                            index += 1;
                            let escaped = *bytes
                                .get(index)
                                .ok_or(VdfError::UnterminatedString(start))?;
                            value.push(match escaped {
                                b'n' => '\n',
                                b'r' => '\r',
                                b't' => '\t',
                                b'"' => '"',
                                b'\\' => '\\',
                                other => other as char,
                            });
                            index += 1;
                        }
                        byte => {
                            value.push(byte as char);
                            index += 1;
                        }
                    }
                }
                if !closed {
                    return Err(VdfError::UnterminatedString(start));
                }
                tokens.push(Token::Text(value));
            }
            _ => {
                let start = index;
                while index < bytes.len()
                    && !matches!(bytes[index], b' ' | b'\t' | b'\r' | b'\n' | b'{' | b'}')
                {
                    index += 1;
                }
                if index == start {
                    return Err(VdfError::UnexpectedToken(index));
                }
                tokens.push(Token::Text(input[start..index].to_string()));
            }
        }
    }

    Ok(tokens)
}

fn parse_object(
    tokens: &[Token],
    cursor: &mut usize,
    requires_close: bool,
) -> Result<BTreeMap<String, VdfValue>, VdfError> {
    let mut object = BTreeMap::new();
    while *cursor < tokens.len() {
        if matches!(tokens[*cursor], Token::Close) {
            if !requires_close {
                return Err(VdfError::UnexpectedToken(*cursor));
            }
            *cursor += 1;
            return Ok(object);
        }

        let key = match tokens.get(*cursor) {
            Some(Token::Text(value)) => value.clone(),
            Some(_) => return Err(VdfError::UnexpectedToken(*cursor)),
            None => return Err(VdfError::UnexpectedEnd),
        };
        *cursor += 1;

        let value = match tokens.get(*cursor) {
            Some(Token::Text(value)) => {
                *cursor += 1;
                VdfValue::Text(value.clone())
            }
            Some(Token::Open) => {
                *cursor += 1;
                VdfValue::Object(parse_object(tokens, cursor, true)?)
            }
            Some(_) => return Err(VdfError::UnexpectedToken(*cursor)),
            None => return Err(VdfError::UnexpectedEnd),
        };
        object.insert(key, value);
    }

    if requires_close {
        Err(VdfError::UnclosedObject)
    } else {
        Ok(object)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_comments_escapes_and_nested_objects() {
        let parsed = parse(
            r#"
            // launcher state
            "libraryfolders"
            {
                "0" { "path" "C:\\Games\\Steam" }
            }
            "#,
        )
        .expect("valid VDF");
        let path = parsed["libraryfolders"].as_object().unwrap()["0"]
            .as_object()
            .unwrap()["path"]
            .as_text()
            .unwrap();
        assert_eq!(path, r"C:\Games\Steam");
    }

    #[test]
    fn rejects_unclosed_objects() {
        assert_eq!(
            parse(r#""root" { "value" "x""#),
            Err(VdfError::UnclosedObject)
        );
    }
}
