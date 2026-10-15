//! Quad9への問い合わせは、検証済みのASCIIホスト名だけをDNS本文へ入れる。
//! 対象ページやCNAME先をHTTPで取得せず、URL・認証情報・パスは受け取らない。

use std::time::Duration;

const MAX_DNS_MESSAGE: usize = 65_535;
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reputation {
    /// Quad9のブロック応答ではない。サイトの安全性を保証する結果ではない。
    Clear,
    Blocked,
    Unavailable(String),
}

pub fn lookup(domain: &str) -> Reputation {
    let domain = match public_domain(domain) {
        Ok(domain) => domain,
        Err(reason) => return Reputation::Unavailable(reason.into()),
    };
    // WinHTTPの各I/Oにも制限を置き、プロキシ探索などが遅い場合も画面へ戻す。
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    if std::thread::Builder::new()
        .name("quad9-domain-check".into())
        .spawn(move || {
            let result = perform_lookup(&domain);
            let _ = sender.send(result);
        })
        .is_err()
    {
        return Reputation::Unavailable("危険ドメイン照会を開始できませんでした。".into());
    }
    receiver.recv_timeout(LOOKUP_TIMEOUT).unwrap_or_else(|_| {
        Reputation::Unavailable("Quad9への照会が制限時間内に完了しませんでした。".into())
    })
}

fn public_domain(domain: &str) -> Result<String, &'static str> {
    // URLそのもの、IP、単一ラベル名、予約・ローカル名を送信しない。
    // 組織独自のDNS名を完全に識別することはできないため、呼出側でも判定する。
    let domain = domain.strip_suffix('.').unwrap_or(domain);
    if !domain.is_ascii() || domain.len() > 253 || domain.parse::<std::net::IpAddr>().is_ok() {
        return Err("外部照会できる公開ドメインではありません。");
    }
    let domain = domain.to_ascii_lowercase();
    let labels: Vec<_> = domain.split('.').collect();
    if labels.len() < 2
        || labels.iter().any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    {
        return Err("外部照会できる公開ドメインではありません。");
    }
    let tld = labels.last().copied().unwrap_or_default();
    if tld.len() < 2
        || (!tld.bytes().all(|byte| byte.is_ascii_alphabetic()) && !tld.starts_with("xn--"))
        || [
            "localhost",
            "local",
            "localdomain",
            "internal",
            "intranet",
            "lan",
            "home",
            "corp",
            "test",
            "invalid",
            "example",
            "onion",
            "arpa",
        ]
        .contains(&tld)
    {
        return Err("ローカル名または予約ドメインは外部へ送信しません。");
    }
    if !psl::suffix(domain.as_bytes()).is_some_and(|suffix| suffix.is_known()) {
        return Err("公開ドメインとして確認できない名前は外部へ送信しません。");
    }
    Ok(domain)
}

fn dns_query(domain: &str) -> Vec<u8> {
    // RFC 8484: HTTPSではIDを0とし、問い合わせ一つだけを送る。ECSは送らない。
    let mut message = vec![0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0];
    for label in domain.split('.') {
        message.push(label.len() as u8);
        message.extend_from_slice(label.as_bytes());
    }
    message.extend_from_slice(&[0, 0, 1, 0, 1]); // QTYPE=A / QCLASS=IN
    message
}

fn perform_lookup(domain: &str) -> Reputation {
    let query = dns_query(domain);
    match transport::post_dns(&query).and_then(|response| parse_response(&response, domain)) {
        Ok(reputation) => reputation,
        Err(reason) => Reputation::Unavailable(reason),
    }
}

fn parse_response(message: &[u8], domain: &str) -> Result<Reputation, String> {
    if !(12..=MAX_DNS_MESSAGE).contains(&message.len()) {
        return Err("Quad9のDNS応答サイズが不正です。".into());
    }
    let mut cursor = 0;
    let id = read_u16(message, &mut cursor)?;
    let flags = read_u16(message, &mut cursor)?;
    let questions = read_u16(message, &mut cursor)?;
    let answers = read_u16(message, &mut cursor)?;
    let authorities = read_u16(message, &mut cursor)?;
    let additional = read_u16(message, &mut cursor)?;
    // QR=応答、OPCODE=QUERY、非切詰、Z=0、再帰要求の応答を受け付ける。
    if id != 0 || flags & 0xf840 != 0x8000 || flags & 0x0200 != 0 || flags & 0x0100 == 0 {
        return Err("Quad9のDNS応答ヘッダーが不正です。".into());
    }
    if questions != 1 {
        return Err("Quad9のDNS応答の質問数が不正です。".into());
    }
    let question = read_name(message, &mut cursor)?;
    let expected: Vec<Vec<u8>> = domain
        .split('.')
        .map(|label| label.as_bytes().to_ascii_lowercase())
        .collect();
    if question != expected
        || read_u16(message, &mut cursor)? != 1
        || read_u16(message, &mut cursor)? != 1
    {
        return Err("Quad9のDNS応答が照会したドメインと一致しません。".into());
    }
    let mut soa_in_authority = false;
    let mut seen_opt = false;
    for (section, count) in [answers, authorities, additional].into_iter().enumerate() {
        for _ in 0..count {
            let owner = read_name(message, &mut cursor)?;
            let record_type = read_u16(message, &mut cursor)?;
            let class = read_u16(message, &mut cursor)?;
            let ttl = read_u32(message, &mut cursor)?;
            let length = usize::from(read_u16(message, &mut cursor)?);
            let end = cursor
                .checked_add(length)
                .filter(|end| *end <= message.len())
                .ok_or("Quad9のDNSレコード長が不正です。")?;
            if record_type == 41 {
                // EDNSの拡張エラーや未対応版を通常のNOERRORとして扱わない。
                if section != 2 || seen_opt || !owner.is_empty() || ttl >> 16 != 0 {
                    return Err("Quad9のEDNS応答が不正または未対応です。".into());
                }
                seen_opt = true;
                while cursor < end {
                    let _code = read_u16(message, &mut cursor)?;
                    let option_length = usize::from(read_u16(message, &mut cursor)?);
                    cursor = cursor
                        .checked_add(option_length)
                        .filter(|position| *position <= end)
                        .ok_or("Quad9のEDNSオプション長が不正です。")?;
                }
            } else {
                if class != 1 {
                    return Err("Quad9のDNSレコードクラスが不正です。".into());
                }
                validate_record_data(message, &mut cursor, end, record_type)?;
                soa_in_authority |= section == 1 && record_type == 6;
            }
            if cursor != end {
                return Err("Quad9のDNSレコード末尾が不正です。".into());
            }
        }
    }
    if cursor != message.len() {
        return Err("Quad9のDNS応答に余分なデータがあります。".into());
    }
    match flags & 0x000f {
        0 => Ok(Reputation::Clear),
        3 if authorities == 0 => Ok(Reputation::Blocked),
        3 if soa_in_authority => Ok(Reputation::Clear),
        3 => Err("Quad9の不存在応答を確認できませんでした。".into()),
        _ => Err("Quad9がDNS照会エラーを返しました。".into()),
    }
}

fn validate_record_data(
    message: &[u8],
    cursor: &mut usize,
    end: usize,
    record_type: u16,
) -> Result<(), String> {
    let length = end - *cursor;
    match record_type {
        1 if length != 4 => return Err("Quad9のIPv4レコード長が不正です。".into()),
        28 if length != 16 => return Err("Quad9のIPv6レコード長が不正です。".into()),
        2 | 5 | 12 | 39 => {
            let _name = read_name(message, cursor)?;
        }
        6 => {
            let _primary = read_name(message, cursor)?;
            let _mailbox = read_name(message, cursor)?;
            *cursor = cursor
                .checked_add(20)
                .ok_or("Quad9のSOAレコード長が不正です。")?;
        }
        15 | 33 => {
            let prefix = if record_type == 15 { 2 } else { 6 };
            *cursor = cursor
                .checked_add(prefix)
                .filter(|position| *position <= end)
                .ok_or("Quad9のDNSレコード長が不正です。")?;
            let _target = read_name(message, cursor)?;
        }
        _ => *cursor = end,
    }
    if *cursor != end {
        return Err("Quad9のDNSレコードの内容と長さが一致しません。".into());
    }
    Ok(())
}

fn read_u16(message: &[u8], cursor: &mut usize) -> Result<u16, String> {
    let bytes = message
        .get(*cursor..*cursor + 2)
        .ok_or("Quad9のDNS応答が途中で終わっています。")?;
    *cursor += 2;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn read_u32(message: &[u8], cursor: &mut usize) -> Result<u32, String> {
    let bytes = message
        .get(*cursor..*cursor + 4)
        .ok_or("Quad9のDNS応答が途中で終わっています。")?;
    *cursor += 4;
    Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_name(message: &[u8], cursor: &mut usize) -> Result<Vec<Vec<u8>>, String> {
    let mut position = *cursor;
    let mut jumped = false;
    let mut visited = Vec::new();
    let mut labels = Vec::new();
    let mut expanded_length = 1; // 終端の0ラベルを含む255バイト制限。
    loop {
        if visited.len() >= 256 || visited.contains(&position) {
            return Err("Quad9のDNS名の圧縮参照が循環しています。".into());
        }
        visited.push(position);
        let length = *message
            .get(position)
            .ok_or("Quad9のDNS名が途中で終わっています。")?;
        if length & 0xc0 == 0xc0 {
            let low = *message
                .get(position + 1)
                .ok_or("Quad9のDNS名の圧縮参照が不正です。")?;
            let pointer = ((usize::from(length) & 0x3f) << 8) | usize::from(low);
            if pointer < 12 || pointer >= position {
                return Err("Quad9のDNS名の圧縮参照先が不正です。".into());
            }
            if !jumped {
                *cursor = position + 2;
                jumped = true;
            }
            position = pointer;
        } else if length & 0xc0 != 0 {
            return Err("Quad9のDNS名のラベル形式が不正です。".into());
        } else {
            position += 1;
            if length == 0 {
                if !jumped {
                    *cursor = position;
                }
                return Ok(labels);
            }
            let end = position + usize::from(length);
            let label = message
                .get(position..end)
                .ok_or("Quad9のDNS名のラベル長が不正です。")?;
            expanded_length += usize::from(length) + 1;
            if expanded_length > 255 {
                return Err("Quad9のDNS名が長すぎます。".into());
            }
            labels.push(label.to_ascii_lowercase());
            position = end;
        }
    }
}

#[cfg(windows)]
mod transport {
    use super::MAX_DNS_MESSAGE;
    use std::ffi::c_void;
    use std::ptr::{null, null_mut};
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Networking::WinHttp::*;

    const IO_TIMEOUT_MS: i32 = 4_000;

    struct InternetHandle(*mut c_void);

    impl InternetHandle {
        fn new(handle: *mut c_void) -> Result<Self, String> {
            if handle.is_null() {
                Err(last_error())
            } else {
                Ok(Self(handle))
            }
        }

        fn option(&self, option: u32, value: u32) -> Result<(), String> {
            checked(unsafe {
                WinHttpSetOption(
                    self.0,
                    option,
                    (&value as *const u32).cast(),
                    size_of::<u32>() as u32,
                )
            })
        }
    }

    impl Drop for InternetHandle {
        fn drop(&mut self) {
            unsafe { WinHttpCloseHandle(self.0) };
        }
    }

    pub(super) fn post_dns(query: &[u8]) -> Result<Vec<u8>, String> {
        let started = Instant::now();
        let agent = wide("HttpSelector domain safety check");
        let server = wide("dns.quad9.net");
        let verb = wide("POST");
        let path = wide("/dns-query");
        let headers = wide(
            "Content-Type: application/dns-message\r\nAccept: application/dns-message\r\nAccept-Encoding: identity\r\n",
        );
        // Windows設定のプロキシを利用する。TLS検証を緩めるオプションは設定しない。
        let session = InternetHandle::new(unsafe {
            WinHttpOpen(
                agent.as_ptr(),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                null(),
                null(),
                0,
            )
        })?;
        checked(unsafe {
            WinHttpSetTimeouts(
                session.0,
                IO_TIMEOUT_MS,
                IO_TIMEOUT_MS,
                IO_TIMEOUT_MS,
                IO_TIMEOUT_MS,
            )
        })?;
        session.option(
            WINHTTP_OPTION_ENABLE_HTTP_PROTOCOL,
            WINHTTP_PROTOCOL_FLAG_HTTP2,
        )?;
        session.option(
            WINHTTP_OPTION_SECURE_PROTOCOLS,
            WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2 | WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3,
        )?;
        let connection =
            InternetHandle::new(unsafe { WinHttpConnect(session.0, server.as_ptr(), 443, 0) })?;
        let request = InternetHandle::new(unsafe {
            WinHttpOpenRequest(
                connection.0,
                verb.as_ptr(),
                path.as_ptr(),
                null(),
                null(),
                null(),
                WINHTTP_FLAG_SECURE,
            )
        })?;
        request.option(
            WINHTTP_OPTION_REDIRECT_POLICY,
            WINHTTP_OPTION_REDIRECT_POLICY_NEVER,
        )?;
        request.option(
            WINHTTP_OPTION_DISABLE_FEATURE,
            WINHTTP_DISABLE_COOKIES | WINHTTP_DISABLE_AUTHENTICATION,
        )?;
        request.option(WINHTTP_OPTION_MAX_RESPONSE_HEADER_SIZE, 16_384)?;
        checked(unsafe {
            WinHttpSendRequest(
                request.0,
                headers.as_ptr(),
                u32::MAX,
                query.as_ptr().cast(),
                query.len() as u32,
                query.len() as u32,
                0,
            )
        })?;
        checked(unsafe { WinHttpReceiveResponse(request.0, null_mut()) })?;
        let mut protocol = 0_u32;
        let mut protocol_length = size_of::<u32>() as u32;
        checked(unsafe {
            WinHttpQueryOption(
                request.0,
                WINHTTP_OPTION_HTTP_PROTOCOL_USED,
                (&mut protocol as *mut u32).cast(),
                &mut protocol_length,
            )
        })?;
        if protocol & WINHTTP_PROTOCOL_FLAG_HTTP2 == 0 {
            return Err("Quad9とのHTTP/2通信を確認できませんでした。".into());
        }
        let mut status = 0_u32;
        let mut status_length = size_of::<u32>() as u32;
        checked(unsafe {
            WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                null(),
                (&mut status as *mut u32).cast(),
                &mut status_length,
                null_mut(),
            )
        })?;
        if status != 200 {
            return Err(format!(
                "Quad9への照会がHTTPエラーになりました（{status}）。"
            ));
        }
        let content_type = header(&request, WINHTTP_QUERY_CONTENT_TYPE)?;
        if !content_type
            .trim()
            .eq_ignore_ascii_case("application/dns-message")
        {
            return Err("Quad9の応答形式を確認できませんでした。".into());
        }
        let mut response = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            if started.elapsed() >= Duration::from_secs(5) {
                return Err("Quad9の応答受信が制限時間を超えました。".into());
            }
            let mut received = 0_u32;
            checked(unsafe {
                WinHttpReadData(
                    request.0,
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    &mut received,
                )
            })?;
            if received == 0 {
                break;
            }
            if response.len() + received as usize > MAX_DNS_MESSAGE {
                return Err("Quad9の応答がサイズ制限を超えました。".into());
            }
            response.extend_from_slice(&buffer[..received as usize]);
        }
        Ok(response)
    }

    fn header(request: &InternetHandle, query: u32) -> Result<String, String> {
        let mut value = [0_u16; 256];
        let mut byte_length = size_of_val(&value) as u32;
        checked(unsafe {
            WinHttpQueryHeaders(
                request.0,
                query,
                null(),
                value.as_mut_ptr().cast(),
                &mut byte_length,
                null_mut(),
            )
        })?;
        if byte_length % 2 != 0 || byte_length as usize > size_of_val(&value) {
            return Err("Quad9のHTTPヘッダーが不正です。".into());
        }
        let length = value
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(value.len());
        String::from_utf16(&value[..length])
            .map_err(|_| "Quad9のHTTPヘッダーの文字コードが不正です。".into())
    }

    fn checked(result: i32) -> Result<(), String> {
        if result == 0 {
            Err(last_error())
        } else {
            Ok(())
        }
    }

    fn last_error() -> String {
        format!(
            "Quad9への安全な接続を確認できませんでした（{}）。",
            std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
        )
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(Some(0)).collect()
    }
}

#[cfg(not(windows))]
mod transport {
    pub(super) fn post_dns(_query: &[u8]) -> Result<Vec<u8>, String> {
        Err("この環境ではQuad9への照会を利用できません。".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(flags: u16, answers: u16, authority: u16, additional: u16) -> Vec<u8> {
        let mut message = dns_query("example.com");
        message[2..4].copy_from_slice(&flags.to_be_bytes());
        message[6..8].copy_from_slice(&answers.to_be_bytes());
        message[8..10].copy_from_slice(&authority.to_be_bytes());
        message[10..12].copy_from_slice(&additional.to_be_bytes());
        message
    }

    fn record(message: &mut Vec<u8>, record_type: u16, data: &[u8]) {
        message.extend_from_slice(&[0xc0, 0x0c]); // 質問名の圧縮参照。
        message.extend_from_slice(&record_type.to_be_bytes());
        message.extend_from_slice(&[0, 1, 0, 0, 0, 60]);
        message.extend_from_slice(&(data.len() as u16).to_be_bytes());
        message.extend_from_slice(data);
    }

    #[test]
    fn query_contains_only_one_domain_question_without_url_data() {
        let domain = public_domain("Login.Example.com.").unwrap();
        assert_eq!(domain, "login.example.com");
        assert_eq!(
            dns_query(&domain),
            b"\x00\x00\x01\x00\x00\x01\x00\x00\x00\x00\x00\x00\x05login\x07example\x03com\x00\x00\x01\x00\x01"
        );
        for invalid in [
            "https://example.com/secret?token=x#private",
            "example.com/path",
            "u@example.com",
            "example.com:443",
            "example.com?key",
            "example.com#key",
            "example.com\r\nInjected",
            "127.0.0.1",
            "8.8.8.8",
            "[::1]",
            "::1",
            "localhost",
            "printer.local",
            "internal.example",
            "server.home.arpa",
            "foo.test",
            "foo.invalid",
            "foo.onion",
            "bad..com",
            "-bad.com",
            "bad-.com",
            "bad_.com",
            "全角.com",
            "foo.123",
            "server.not-a-public-tld",
        ] {
            assert!(public_domain(invalid).is_err(), "{invalid}");
        }
        assert!(public_domain(&format!("{}.com", "a".repeat(64))).is_err());
    }

    #[test]
    fn normal_and_no_data_replies_are_clear_not_safety_guarantees() {
        let mut message = reply(0x8180, 1, 0, 0);
        record(&mut message, 1, &[192, 0, 2, 1]);
        assert_eq!(
            parse_response(&message, "example.com"),
            Ok(Reputation::Clear)
        );
        assert_eq!(
            parse_response(&reply(0x8180, 0, 0, 0), "example.com"),
            Ok(Reputation::Clear)
        );
    }

    #[test]
    fn nxdomain_without_authority_is_quad9_block() {
        assert_eq!(
            parse_response(&reply(0x8183, 0, 0, 0), "example.com"),
            Ok(Reputation::Blocked)
        );
    }

    #[test]
    fn nxdomain_with_soa_is_nonexistent_not_blocked() {
        let mut message = reply(0x8183, 0, 1, 0);
        let mut soa = vec![0xc0, 0x0c, 0xc0, 0x0c];
        soa.extend_from_slice(&[0; 20]);
        record(&mut message, 6, &soa);
        assert_eq!(
            parse_response(&message, "example.com"),
            Ok(Reputation::Clear)
        );
        let mut invalid = reply(0x8183, 0, 1, 0);
        record(&mut invalid, 2, &[0xc0, 0x0c]);
        assert!(parse_response(&invalid, "example.com").is_err());
    }

    #[test]
    fn servfail_and_other_dns_errors_are_unavailable() {
        for rcode in [1, 2, 4, 5, 15] {
            assert!(parse_response(&reply(0x8180 | rcode, 0, 0, 0), "example.com").is_err());
        }
    }

    #[test]
    fn header_and_question_must_match_request() {
        let original = reply(0x8180, 0, 0, 0);
        for (offset, value) in [
            (0, 1),
            (2, 0x01),
            (2, 0x89),
            (2, 0x83),
            (3, 0xc0),
            (5, 0),
            (13, b'z'),
        ] {
            let mut message = original.clone();
            message[offset] = value;
            assert!(
                parse_response(&message, "example.com").is_err(),
                "offset {offset}"
            );
        }
        let mut wrong_type = original.clone();
        let position = wrong_type.len() - 3;
        wrong_type[position] = 28;
        assert!(parse_response(&wrong_type, "example.com").is_err());
        let mut wrong_class = original;
        let position = wrong_class.len() - 1;
        wrong_class[position] = 3;
        assert!(parse_response(&wrong_class, "example.com").is_err());
    }

    #[test]
    fn cname_compression_is_parsed_without_following_http_targets() {
        let mut message = reply(0x8180, 2, 0, 0);
        record(&mut message, 5, &[3, b'c', b'd', b'n', 0xc0, 0x0c]);
        record(&mut message, 1, &[192, 0, 2, 2]);
        assert_eq!(
            parse_response(&message, "example.com"),
            Ok(Reputation::Clear)
        );
    }

    #[test]
    fn broken_names_lengths_and_trailing_data_are_unavailable() {
        let original = reply(0x8180, 1, 0, 0);
        for data in [
            vec![0xc0],
            vec![0xff, 0xff],
            vec![0xc0, 0x00],
            vec![0x40],
            vec![3, b'a'],
            vec![0, 0],
        ] {
            let mut message = original.clone();
            record(&mut message, 5, &data);
            assert!(parse_response(&message, "example.com").is_err(), "{data:?}");
        }
        let mut forward = original.clone();
        let position = forward.len();
        forward.extend_from_slice(&[0xc0, (position + 2) as u8]);
        assert!(parse_response(&forward, "example.com").is_err());
        let mut looped = vec![0; 12];
        looped.extend_from_slice(&[1, b'x', 0xc0, 12]);
        assert!(read_name(&looped, &mut 12).is_err());
        for length in [0, 11, 12, 13, 28] {
            assert!(parse_response(&original[..length], "example.com").is_err());
        }
        let mut trailing = reply(0x8180, 0, 0, 0);
        trailing.push(0);
        assert!(parse_response(&trailing, "example.com").is_err());
    }

    #[test]
    fn malformed_edns_and_extended_rcode_are_not_clear() {
        let mut message = reply(0x8183, 0, 0, 1);
        message.extend_from_slice(&[0, 0, 41, 4, 208, 0, 0, 0, 0, 0, 0]);
        assert_eq!(
            parse_response(&message, "example.com"),
            Ok(Reputation::Blocked)
        );
        let ttl_start = message.len() - 6;
        message[ttl_start] = 1;
        assert!(parse_response(&message, "example.com").is_err());
        let mut truncated = reply(0x8180, 0, 0, 1);
        truncated.extend_from_slice(&[0, 0, 41, 4, 208, 0, 0, 0, 0, 0, 1, 0]);
        assert!(parse_response(&truncated, "example.com").is_err());
    }

    #[test]
    #[ignore = "Quad9への実通信。安全なexample.comと公式ブロック確認用ドメインのDNSだけを照会する"]
    fn live_quad9_http2_checks_safe_and_official_blocked_domains() {
        assert_eq!(lookup("example.com"), Reputation::Clear);
        assert_eq!(lookup("isitblocked.org"), Reputation::Blocked);
    }
}
