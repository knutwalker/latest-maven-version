use std::iter::FromIterator;
use xmlparser::{ElementEnd as EE, Error, Token, Tokenizer};

pub(crate) struct Metadata {
    doc: String,
}

impl Metadata {
    #[inline]
    pub(crate) fn iter<'a>(&'a self) -> MetadataParser<'a> {
        self.into_iter()
    }

    pub(crate) fn parse_into<T>(input: String) -> Result<T, Error>
    where
        T: for<'a> FromIterator<&'a str>,
    {
        let parser = Self::from(input);
        parser.iter().collect::<Result<T, Error>>()
    }
}

impl<T: Into<String>> From<T> for Metadata {
    fn from(value: T) -> Self {
        Metadata { doc: value.into() }
    }
}

impl<'a> IntoIterator for &'a Metadata {
    type Item = Result<&'a str, Error>;

    type IntoIter = MetadataParser<'a>;

    fn into_iter(self) -> Self::IntoIter {
        MetadataParser::from(&self.doc[..])
    }
}

pub(crate) struct MetadataParser<'a> {
    tok: Tokenizer<'a>,
    state: State,
}

#[derive(Debug, Copy, Clone)]
#[repr(u8)]
enum State {
    ExpectFirstVersionStart,
    ExpectVersionEnd,
    ExpectNextVersionStart,
    ExpectVersion,
    Eoi,
}

impl<'a> From<&'a str> for MetadataParser<'a> {
    fn from(input: &'a str) -> Self {
        MetadataParser {
            tok: Tokenizer::from(input),
            state: State::ExpectFirstVersionStart,
        }
    }
}

impl<'a> MetadataParser<'a> {
    #[cfg(test)]
    fn parse_into<T>(input: &'a str) -> Result<T, Error>
    where
        T: FromIterator<&'a str>,
    {
        let parser = Self::from(input);
        parser.collect::<Result<T, Error>>()
    }
}

const VERSION_TAG: &str = "version";

impl<'a> Iterator for MetadataParser<'a> {
    type Item = Result<&'a str, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if let State::Eoi = self.state {
            return None;
        }

        while let Some(token) = self.tok.next() {
            let token = match token {
                Ok(token) => token,
                Err(e) => return Some(Err(e)),
            };
            match self.state {
                State::ExpectFirstVersionStart => match token {
                    Token::ElementStart { local, .. } if local.as_str() == VERSION_TAG => {
                        self.state = State::ExpectVersionEnd;
                    }
                    _ => {}
                },
                State::ExpectNextVersionStart => match token {
                    Token::ElementStart { local, .. } if local.as_str() == VERSION_TAG => {
                        self.state = State::ExpectVersionEnd;
                    }
                    Token::ElementEnd {
                        end: EE::Close(_, _),
                        ..
                    } => {
                        self.state = State::Eoi;
                        break;
                    }
                    _ => {}
                },
                State::ExpectVersionEnd => match token {
                    Token::ElementEnd { end: EE::Open, .. } => {
                        self.state = State::ExpectVersion;
                    }
                    _ => {}
                },
                State::ExpectVersion => match token {
                    Token::Text { text } => return Some(Ok(text.as_str().trim())),
                    Token::Cdata { text, .. } => return Some(Ok(text.as_str().trim())),
                    Token::ElementEnd {
                        end: EE::Close(_, _),
                        ..
                    } => {
                        self.state = State::ExpectNextVersionStart;
                    }
                    _ => {}
                },
                State::Eoi => break,
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

    #[test_case(""; "empty string")]
    #[test_case("<metadata></metadata>"; "unrelated tag")]
    #[test_case("<versions></versions>"; "versions without version")]
    #[test_case("<version></version>"; "version without versions")]
    #[test_case("<versions><version></version></versions>"; "version without content")]
    fn test_empty_xml(input: &str) {
        let versions = MetadataParser::parse_into::<Vec<_>>(input).unwrap();
        assert_eq!(versions, Vec::<&str>::new());
    }

    #[test_case("<versions><version>   </version></versions>" => vec![""]; "whitespace only")]
    #[test_case("<versions><version>1.0.0</version></versions>" => vec!["1.0.0"]; "1.0.0")]
    #[test_case("<versions><version>   1.0.0   </version></versions>" => vec!["1.0.0"]; "1.0.0 with whitespace")]
    #[test_case("<versions><version><![CDATA[1.0.0]]></version></versions>" => vec!["1.0.0"]; "1.0.0 in CDATA")]
    #[test_case("<versions><version><![CDATA[   1.0.0    ]]></version></versions>" => vec!["1.0.0"]; "1.0.0 in CDATA with whitespace")]
    #[test_case("<versions><version>foo</version></versions>" => vec!["foo"]; "accepts anything")]
    fn test_minimal_xml(input: &str) -> Vec<&str> {
        MetadataParser::parse_into(input).unwrap()
    }

    #[test]
    fn test_full_xml() {
        let input = r#"<?xml version="1.0" encoding="UTF-8"?>
        <metadata>
          <groupId>org.neo4j.gds</groupId>
          <artifactId>proc</artifactId>
          <versioning>
            <latest>1.4.0-alpha03</latest>
            <release>1.4.0-alpha03</release>
            <versions>
              <version>0.9.2</version>
              <version>0.9.3</version>
              <version>1.0.0</version>
              <version>1.1.0-alpha01</version>
              <version>1.1.0-alpha02</version>
              <version>1.1.0</version>
              <version>1.1.1</version>
              <version>1.1.2</version>
              <version>1.1.3</version>
              <version>1.1.4</version>
              <version>1.1.5</version>
              <version>1.2.0-alpha01</version>
              <version>1.2.0</version>
              <version>1.2.1</version>
              <version>1.2.2</version>
              <version>1.2.3</version>
              <version>1.3.0-alpha01</version>
              <version>1.3.0-alpha02</version>
              <version>1.3.0-alpha03</version>
              <version>1.3.0</version>
              <version>1.3.1</version>
              <version>1.3.2</version>
              <version>1.4.0-alpha01</version>
              <version>1.4.0-alpha02</version>
              <version>1.4.0-alpha03</version>
            </versions>
            <lastUpdated>20200827153717</lastUpdated>
          </versioning>
        </metadata>
        "#;

        let versions = MetadataParser::parse_into::<Vec<_>>(input).unwrap();
        assert_eq!(
            versions,
            vec![
                "0.9.2",
                "0.9.3",
                "1.0.0",
                "1.1.0-alpha01",
                "1.1.0-alpha02",
                "1.1.0",
                "1.1.1",
                "1.1.2",
                "1.1.3",
                "1.1.4",
                "1.1.5",
                "1.2.0-alpha01",
                "1.2.0",
                "1.2.1",
                "1.2.2",
                "1.2.3",
                "1.3.0-alpha01",
                "1.3.0-alpha02",
                "1.3.0-alpha03",
                "1.3.0",
                "1.3.1",
                "1.3.2",
                "1.4.0-alpha01",
                "1.4.0-alpha02",
                "1.4.0-alpha03"
            ]
        );
    }
}
