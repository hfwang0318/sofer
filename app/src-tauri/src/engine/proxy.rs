//! Download proxy URLs: composition from the settings fields, migration of the
//! legacy full URL, and `ureq` proxy resolution for one request.

/// Build a proxy URL from the three settings fields. Usernames/passwords are
/// percent-encoded as URL userinfo, so `p@ss:word` becomes `p%40ss%3Aword`
/// before it is appended to the address.
pub fn compose_proxy_url(
    address: &str,
    username: &str,
    password: &str,
) -> Result<Option<String>, String> {
    let address = address.trim();
    if address.is_empty() {
        if username.is_empty() && password.is_empty() {
            return Ok(None);
        }
        return Err("填写了代理认证信息，但代理地址为空。".into());
    }
    if username.is_empty() && !password.is_empty() {
        return Err("填写代理密码前，请先填写用户名。".into());
    }
    if username.chars().any(char::is_control) || password.chars().any(char::is_control) {
        return Err("代理用户名/密码不能包含换行或其他控制字符。".into());
    }

    let with_scheme = if address.contains("://") {
        address.to_string()
    } else {
        format!("http://{address}")
    };
    let Some((scheme, rest)) = with_scheme.split_once("://") else {
        return Err("代理地址格式无效。".into());
    };
    if !scheme.eq_ignore_ascii_case("http") {
        return Err("下载代理目前只支持 http:// 地址。".into());
    }
    if rest.contains('@') {
        return Err("代理地址里不要再写用户名/密码，请填到下面的认证字段。".into());
    }
    let host = rest.trim_end_matches('/');
    if host.is_empty()
        || host.contains('/')
        || host.contains('?')
        || host.contains('#')
        || host.chars().any(char::is_whitespace)
    {
        return Err("代理地址只填 host:port，例如 127.0.0.1:7890。".into());
    }
    if username.is_empty() && password.is_empty() {
        return Ok(Some(format!("http://{host}")));
    }
    let userinfo = format!(
        "{}:{}",
        percent_encode_userinfo(username),
        percent_encode_userinfo(password)
    );
    Ok(Some(format!("http://{userinfo}@{host}")))
}

/// Split a legacy full proxy URL (`http://user:pass@host:port`) into its parts.
/// Used only to migrate pre-existing settings files.
pub fn split_proxy_url(raw: &str) -> Option<(String, String, String)> {
    let value = raw.trim();
    let with_scheme = if value.contains("://") {
        value.to_string()
    } else {
        format!("http://{value}")
    };
    let (scheme, rest) = with_scheme.split_once("://")?;
    let (userinfo, host) = rest.rsplit_once('@')?;
    let (username, password) = userinfo.split_once(':').unwrap_or((userinfo, ""));
    if host.trim().is_empty() {
        return None;
    }
    Some((
        format!("{scheme}://{}", host.trim_end_matches('/')),
        percent_decode(username),
        percent_decode(password),
    ))
}

fn percent_encode_userinfo(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Many developer machines reach Hugging Face through a local proxy. `ureq`
/// does not read the conventional environment variables itself, so honour them
/// for the download path only (local health checks stay direct).
/// `ureq` does not percent-decode proxy credentials. Users (and URL standards)
/// usually encode `@` as `%40`, so decode the userinfo before handing the URL to
/// `ureq`; its parser deliberately accepts a raw `@` in the password.
pub(crate) fn normalize_proxy_url(raw: &str) -> String {
    let value = raw.trim();
    let Some((scheme, rest)) = value.split_once("://") else {
        return value.to_string();
    };
    let Some(at) = rest.rfind('@') else {
        return value.to_string();
    };
    let (userinfo, host) = rest.split_at(at);
    format!("{scheme}://{}@{}", percent_decode(userinfo), &host[1..])
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex_value(bytes[i + 1]), hex_value(bytes[i + 2])) {
                out.push(high * 16 + low);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub(super) fn proxy_for_url(url: &str, override_proxy: Option<&str>) -> Option<ureq::Proxy> {
    let lower = url.to_lowercase();
    if lower.contains("://127.0.0.1")
        || lower.contains("://localhost")
        || lower.contains("://[::1]")
    {
        return None;
    }
    if let Some(value) = override_proxy
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        match ureq::Proxy::new(normalize_proxy_url(value)) {
            Ok(proxy) => return Some(proxy),
            Err(err) => eprintln!("引擎：忽略无效的下载代理：{err}"),
        }
    }
    proxy_from_env()
}

fn proxy_from_env() -> Option<ureq::Proxy> {
    for key in [
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "HTTP_PROXY",
        "http_proxy",
    ] {
        let Ok(value) = std::env::var(key) else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match ureq::Proxy::new(normalize_proxy_url(value)) {
            Ok(proxy) => return Some(proxy),
            Err(err) => eprintln!("引擎：忽略无效的代理配置 {key}：{err}"),
        }
    }
    None
}
