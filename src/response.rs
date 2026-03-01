use crate::error::{Pop3Error, Result};
use crate::types::{Capability, ListEntry, Stat, UidlEntry};

/// Parse a POP3 status line, returning the text after `+OK` or an error for `-ERR`.
pub(crate) fn parse_status_line(line: &str) -> Result<&str> {
    let line = line.trim_end_matches("\r\n").trim_end_matches('\n');
    if line.starts_with("+OK")
        && (line.len() == 3 || line.as_bytes()[3].is_ascii_whitespace())
    {
        Ok(line[3..].trim_start())
    } else if line.starts_with("-ERR")
        && (line.len() == 4 || line.as_bytes()[4].is_ascii_whitespace())
    {
        Err(Pop3Error::ServerError(line[4..].trim_start().to_string()))
    } else {
        Err(Pop3Error::Parse(format!("unexpected response: {line}")))
    }
}

/// Parse the response to a `STAT` command.
pub(crate) fn parse_stat(status_text: &str) -> Result<Stat> {
    let mut parts = status_text.split_whitespace();
    let count_str = parts
        .next()
        .ok_or_else(|| Pop3Error::Parse("STAT: missing message count".into()))?;
    let size_str = parts
        .next()
        .ok_or_else(|| Pop3Error::Parse("STAT: missing mailbox size".into()))?;
    let message_count: u32 = count_str
        .parse()
        .map_err(|_| Pop3Error::Parse(format!("STAT: invalid message count: {count_str}")))?;
    let mailbox_size: u64 = size_str
        .parse()
        .map_err(|_| Pop3Error::Parse(format!("STAT: invalid mailbox size: {size_str}")))?;
    Ok(Stat {
        message_count,
        mailbox_size,
    })
}

/// Parse a single LIST entry line like `1 1234`.
pub(crate) fn parse_list_entry(line: &str) -> Result<ListEntry> {
    let line = line.trim();
    let mut parts = line.split_whitespace();
    let id_str = parts
        .next()
        .ok_or_else(|| Pop3Error::Parse("LIST: missing message id".into()))?;
    let size_str = parts
        .next()
        .ok_or_else(|| Pop3Error::Parse("LIST: missing message size".into()))?;
    let message_id: u32 = id_str
        .parse()
        .map_err(|_| Pop3Error::Parse(format!("LIST: invalid message id: {id_str}")))?;
    let size: u64 = size_str
        .parse()
        .map_err(|_| Pop3Error::Parse(format!("LIST: invalid message size: {size_str}")))?;
    if parts.next().is_some() {
        return Err(Pop3Error::Parse(format!(
            "LIST: unexpected extra fields: {line}"
        )));
    }
    Ok(ListEntry { message_id, size })
}

/// Parse a multi-line LIST response body (lines between +OK and the dot terminator).
pub(crate) fn parse_list_multi(body: &str) -> Result<Vec<ListEntry>> {
    body.lines()
        .filter(|line| !line.is_empty())
        .map(parse_list_entry)
        .collect()
}

/// Parse a single-message LIST response from the status text (e.g. `1 1234`).
pub(crate) fn parse_list_single(status_text: &str) -> Result<ListEntry> {
    parse_list_entry(status_text)
}

/// Parse a single UIDL entry line like `1 abc123`.
pub(crate) fn parse_uidl_entry(line: &str) -> Result<UidlEntry> {
    let line = line.trim();
    let mut parts = line.split_whitespace();
    let id_str = parts
        .next()
        .ok_or_else(|| Pop3Error::Parse("UIDL: missing message id".into()))?;
    let uid = parts
        .next()
        .ok_or_else(|| Pop3Error::Parse("UIDL: missing unique id".into()))?;
    let message_id: u32 = id_str
        .parse()
        .map_err(|_| Pop3Error::Parse(format!("UIDL: invalid message id: {id_str}")))?;
    if parts.next().is_some() {
        return Err(Pop3Error::Parse(format!(
            "UIDL: unexpected extra fields: {line}"
        )));
    }
    Ok(UidlEntry {
        message_id,
        unique_id: uid.to_string(),
    })
}

/// Parse a multi-line UIDL response body.
pub(crate) fn parse_uidl_multi(body: &str) -> Result<Vec<UidlEntry>> {
    body.lines()
        .filter(|line| !line.is_empty())
        .map(parse_uidl_entry)
        .collect()
}

/// Parse a single-message UIDL response from the status text.
pub(crate) fn parse_uidl_single(status_text: &str) -> Result<UidlEntry> {
    parse_uidl_entry(status_text)
}

/// Parse a CAPA response body into a list of capabilities.
pub(crate) fn parse_capa(body: &str) -> Vec<Capability> {
    body.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            let mut parts = line.split_whitespace();
            let name = parts.next().unwrap_or("").to_string();
            let arguments: Vec<String> = parts.map(|s| s.to_string()).collect();
            Capability { name, arguments }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_status_ok() {
        let result = parse_status_line("+OK server ready\r\n");
        assert_eq!(result.unwrap(), "server ready");
    }

    #[test]
    fn test_parse_status_ok_empty() {
        let result = parse_status_line("+OK\r\n");
        assert_eq!(result.unwrap(), "");
    }

    #[test]
    fn test_parse_status_err() {
        let result = parse_status_line("-ERR invalid password\r\n");
        assert!(result.is_err());
        match result.unwrap_err() {
            Pop3Error::ServerError(msg) => assert_eq!(msg, "invalid password"),
            e => panic!("unexpected error: {e:?}"),
        }
    }

    #[test]
    fn test_parse_status_unexpected() {
        let result = parse_status_line("GARBAGE\r\n");
        assert!(result.is_err());
        match result.unwrap_err() {
            Pop3Error::Parse(_) => {}
            e => panic!("unexpected error: {e:?}"),
        }
    }

    #[test]
    fn test_parse_stat() {
        let stat = parse_stat("5 12345").unwrap();
        assert_eq!(stat.message_count, 5);
        assert_eq!(stat.mailbox_size, 12345);
    }

    #[test]
    fn test_parse_stat_large_size() {
        let stat = parse_stat("1 3000000000").unwrap();
        assert_eq!(stat.message_count, 1);
        assert_eq!(stat.mailbox_size, 3_000_000_000);
    }

    #[test]
    fn test_parse_stat_missing_fields() {
        assert!(parse_stat("").is_err());
        assert!(parse_stat("5").is_err());
    }

    #[test]
    fn test_parse_stat_invalid_number() {
        assert!(parse_stat("abc 123").is_err());
        assert!(parse_stat("5 abc").is_err());
    }

    #[test]
    fn test_parse_list_entry() {
        let entry = parse_list_entry("1 1234").unwrap();
        assert_eq!(entry.message_id, 1);
        assert_eq!(entry.size, 1234);
    }

    #[test]
    fn test_parse_list_multi() {
        let body = "1 1234\n2 5678\n3 91011\n";
        let entries = parse_list_multi(body).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].message_id, 1);
        assert_eq!(entries[0].size, 1234);
        assert_eq!(entries[2].message_id, 3);
        assert_eq!(entries[2].size, 91011);
    }

    #[test]
    fn test_parse_list_single() {
        let entry = parse_list_single("2 5678").unwrap();
        assert_eq!(entry.message_id, 2);
        assert_eq!(entry.size, 5678);
    }

    #[test]
    fn test_parse_uidl_entry() {
        let entry = parse_uidl_entry("1 abc123def").unwrap();
        assert_eq!(entry.message_id, 1);
        assert_eq!(entry.unique_id, "abc123def");
    }

    #[test]
    fn test_parse_uidl_multi() {
        let body = "1 uid-aaa\n2 uid-bbb\n";
        let entries = parse_uidl_multi(body).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].unique_id, "uid-aaa");
        assert_eq!(entries[1].unique_id, "uid-bbb");
    }

    #[test]
    fn test_parse_uidl_single() {
        let entry = parse_uidl_single("3 unique-xyz").unwrap();
        assert_eq!(entry.message_id, 3);
        assert_eq!(entry.unique_id, "unique-xyz");
    }

    #[test]
    fn test_parse_capa() {
        let body = "TOP\nUIDL\nSASL PLAIN LOGIN\nRESP-CODES\n";
        let caps = parse_capa(body);
        assert_eq!(caps.len(), 4);
        assert_eq!(caps[0].name, "TOP");
        assert!(caps[0].arguments.is_empty());
        assert_eq!(caps[2].name, "SASL");
        assert_eq!(caps[2].arguments, vec!["PLAIN", "LOGIN"]);
    }

    #[test]
    fn test_parse_capa_empty() {
        let caps = parse_capa("");
        assert!(caps.is_empty());
    }

    #[test]
    fn test_parse_status_rejects_okay() {
        // "+OKAY" should not match "+OK"
        let result = parse_status_line("+OKAY\r\n");
        assert!(result.is_err());
        match result.unwrap_err() {
            Pop3Error::Parse(_) => {}
            e => panic!("unexpected error: {e:?}"),
        }
    }

    #[test]
    fn test_parse_status_rejects_error() {
        // "-ERROR" should not match "-ERR"
        let result = parse_status_line("-ERROR\r\n");
        assert!(result.is_err());
        match result.unwrap_err() {
            Pop3Error::Parse(_) => {}
            e => panic!("unexpected error: {e:?}"),
        }
    }

    #[test]
    fn test_parse_capa_whitespace_only_lines() {
        let body = "TOP\n   \nUIDL\n";
        let caps = parse_capa(body);
        assert_eq!(caps.len(), 2);
        assert_eq!(caps[0].name, "TOP");
        assert_eq!(caps[1].name, "UIDL");
    }

    #[test]
    fn test_error_display() {
        let err = Pop3Error::ServerError("mailbox locked".into());
        assert_eq!(err.to_string(), "server error: mailbox locked");

        let err = Pop3Error::NotAuthenticated;
        assert_eq!(err.to_string(), "not authenticated");

        let err = Pop3Error::InvalidInput;
        assert_eq!(err.to_string(), "invalid input: CRLF injection detected");
    }
}
