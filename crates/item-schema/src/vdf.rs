//! Minimal VDF (KeyValues) parser for the live schema refresh — just
//! enough for `items_game.txt` and `csgo_english.txt`.

use serde_json::{Map, Value};

/// Parses a VDF document into a JSON value tree. `coerce_numbers` turns
/// integer-looking values into numbers so the result matches the bundled
/// schema's types (e.g. `attributes["cannot trade"]` is read as `u64`);
/// translations keep every value as a string.
pub fn parse(text: &str, coerce_numbers: bool) -> Result<Value, String> {
    let mut parser = Parser {
        chars: text.chars().collect(),
        pos: 0,
        coerce_numbers,
    };
    let mut root = Map::new();
    match parser.key_values(&mut root)? {
        End::Eof => Ok(Value::Object(root)),
        End::CloseBrace => Err("unexpected closing brace in VDF".to_owned()),
    }
}

enum End {
    Eof,
    CloseBrace,
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
    coerce_numbers: bool,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn bump(&mut self) {
        self.pos += 1;
    }

    fn skip_trivia(&mut self) {
        loop {
            match self.peek() {
                Some(character) if character.is_whitespace() => self.bump(),
                Some('/') if self.peek_at(1) == Some('/') => {
                    while let Some(character) = self.peek() {
                        self.bump();
                        if character == '\n' {
                            break;
                        }
                    }
                }
                _ => return,
            }
        }
    }

    /// Reads a quoted or bare token. `None` means end-of-input or a
    /// structural character where a token was expected.
    fn token(&mut self) -> Result<Option<String>, String> {
        self.skip_trivia();
        match self.peek() {
            None => Ok(None),
            Some('"') => {
                self.bump();
                let mut text = String::new();
                loop {
                    match self.chars.get(self.pos).copied() {
                        None => return Err("unterminated string in VDF".to_owned()),
                        Some('\\') => {
                            self.bump();
                            match self.chars.get(self.pos).copied() {
                                None => return Err("unterminated escape in VDF".to_owned()),
                                Some('n') => text.push('\n'),
                                Some('t') => text.push('\t'),
                                Some(character) => text.push(character),
                            }
                            self.bump();
                        }
                        Some('"') => {
                            self.bump();
                            return Ok(Some(text));
                        }
                        Some(character) => {
                            text.push(character);
                            self.bump();
                        }
                    }
                }
            }
            Some(_) => {
                let mut text = String::new();
                while let Some(character) = self.peek() {
                    if character.is_whitespace() || matches!(character, '{' | '}' | '"') {
                        break;
                    }
                    text.push(character);
                    self.bump();
                }
                Ok((!text.is_empty()).then_some(text))
            }
        }
    }

    fn key_values(&mut self, map: &mut Map<String, Value>) -> Result<End, String> {
        loop {
            self.skip_trivia();
            match self.peek() {
                None => return Ok(End::Eof),
                Some('}') => {
                    self.bump();
                    return Ok(End::CloseBrace);
                }
                Some(_) => {}
            }
            let key = self
                .token()?
                .ok_or_else(|| "expected a key in VDF".to_owned())?;
            self.skip_trivia();
            let value = match self.peek() {
                Some('{') => {
                    self.bump();
                    let mut child = Map::new();
                    match self.key_values(&mut child)? {
                        End::CloseBrace => Value::Object(child),
                        End::Eof => return Err(format!("unterminated block for key {key:?}")),
                    }
                }
                Some(_) => {
                    let text = self
                        .token()?
                        .ok_or_else(|| format!("missing value for key {key:?}"))?;
                    if self.coerce_numbers
                        && let Ok(number) = text.parse::<i64>()
                    {
                        Value::Number(number.into())
                    } else {
                        Value::String(text)
                    }
                }
                None => return Err(format!("missing value for key {key:?}")),
            };
            // The live schema repeats section names (`items` appears 111
            // times at the top level), so blocks merge; scalars stay
            // last-wins like legacy's object assignment.
            match (map.remove(&key), value) {
                (Some(Value::Object(existing)), Value::Object(incoming)) => {
                    map.insert(key, Value::Object(merge_objects(existing, incoming)));
                }
                (_, value) => {
                    map.insert(key, value);
                }
            }
        }
    }
}

fn merge_objects(mut left: Map<String, Value>, right: Map<String, Value>) -> Map<String, Value> {
    for (key, value) in right {
        match (left.remove(&key), value) {
            (Some(Value::Object(existing)), Value::Object(incoming)) => {
                left.insert(key, Value::Object(merge_objects(existing, incoming)));
            }
            (_, value) => {
                left.insert(key, value);
            }
        }
    }
    left
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_blocks_bare_tokens_and_comments() {
        let value = parse(
            r#"
            // a comment
            "items_game"
            {
                "items"   // trailing comment
                {
                    "7" { "name" weapon_ak47 "baseitem" "1" }
                }
                "title" "Semi \"quoted\" value"
            }
            "#,
            true,
        )
        .unwrap();
        let items = &value["items_game"]["items"]["7"];
        assert_eq!(items["name"], "weapon_ak47");
        assert_eq!(items["baseitem"], 1);
        assert_eq!(
            value["items_game"]["title"],
            "Semi \"quoted\" value"
        );
    }

    #[test]
    fn number_coercion_can_be_disabled_for_translations() {
        let quoted = parse(r##""Tokens" { "#count" "5" }"##, false).unwrap();
        assert_eq!(quoted["Tokens"]["#count"], "5");

        let coerced = parse(r##""Tokens" { "#count" "5" }"##, true).unwrap();
        assert_eq!(coerced["Tokens"]["#count"], 5);
    }

    #[test]
    fn malformed_documents_are_rejected() {
        assert!(parse(r#""key" { "nested" "value""#, true).is_err());
        assert!(parse(r#"{ "key" "value" }"#, true).is_err());
        assert!(parse(r#""key""#, true).is_err());
    }

    #[test]
    fn duplicate_scalars_keep_the_last_value_like_legacy() {
        let value = parse(r#""a" "first" "a" "second""#, false).unwrap();
        assert_eq!(value["a"], "second");
    }

    #[test]
    fn repeated_blocks_merge_so_no_section_is_lost() {
        // The live items_game repeats section names top to bottom.
        let value = parse(
            r##""items_game"
            {
                "items" { "7" { "name" "weapon_ak47" } }
                "paint_kits" { "44" { "name" "first" } }
                "items" { "8" { "name" "weapon_aug" } }
                "paint_kits" { "44" { "description_tag" "#Finish" } }
            }"##,
            true,
        )
        .unwrap();
        let items = &value["items_game"]["items"];
        assert_eq!(items["7"]["name"], "weapon_ak47");
        assert_eq!(items["8"]["name"], "weapon_aug");
        let paint = &value["items_game"]["paint_kits"]["44"];
        assert_eq!(paint["name"], "first");
        assert_eq!(paint["description_tag"], "#Finish");
    }
}
