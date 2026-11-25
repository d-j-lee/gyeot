//! 얼굴 정적 파일(계약 §7·§12.1). 얼굴 쪽은 인증 없이 주고(비밀이 없다), 보안 머리를 붙인다.

use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use std::path::{Path, PathBuf};

/// 얼굴 쪽 응답의 `Content-Security-Policy`(계약 §7).
pub const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; media-src 'self' data: blob:; connect-src 'self' ipc: http://ipc.localhost; frame-ancestors 'none'; base-uri 'none'; form-action 'none'";

/// 얼굴 쪽 보안 머리. `no_store`면 `Cache-Control: no-store`도(index·얼굴 없음 안내 쪽).
pub fn secure_headers(h: &mut HeaderMap, no_store: bool) {
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(CSP),
    );
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    if no_store {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
}

fn html_response(status: StatusCode, body: String) -> Response {
    let mut r = (status, body).into_response();
    let h = r.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    secure_headers(h, true);
    r
}

/// 얼굴이 없을 때 보이는 안내 쪽.
pub const BUILTIN_PAGE: &str = r#"<!doctype html>
<html lang="ko">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>곁 — 얼굴이 아직 없어요</title>
<style>
body{font-family:system-ui,-apple-system,"Malgun Gothic","Apple SD Gothic Neo",sans-serif;background:#F4EFE6;color:#3A3530;max-width:40rem;margin:3rem auto;padding:0 1.25rem;line-height:1.6}
code,pre{background:#E6DDCF;border-radius:6px;padding:.1rem .35rem}
pre{padding:.75rem 1rem;overflow:auto}
</style>
</head>
<body>
<h1>곁 데몬은 잘 돌고 있어요</h1>
<p>다만 얼굴(화면)이 아직 빌드되지 않았거나, 데몬이 얼굴 폴더를 찾지 못했어요.</p>
<h2>얼굴을 빌드하려면</h2>
<pre>cd face
npm install
npm run build</pre>
<p>빌드한 뒤 데몬을 다시 띄우면 <code>face/dist</code>를 찾아 씁니다.
다른 곳에 두었다면 환경 변수 <code>GYEOT_FACE_DIR</code>에 <code>index.html</code>이 있는 폴더를 적어 주세요.
배포본은 실행 파일 옆의 <code>face</code> 폴더를 씁니다.</p>
<p>상태 확인: <a href="/health"><code>/health</code></a></p>
</body>
</html>
"#;

/// `GET /`·`/index.html`(인증 없음): 얼굴의 `index.html` 그대로, 없으면 얼굴 없음 안내 쪽. 토큰을 싣지 않는다.
pub async fn index_response(face_dir: Option<&Path>) -> Response {
    if let Some(dir) = face_dir {
        match tokio::fs::read(dir.join("index.html")).await {
            Ok(bytes) => {
                return html_response(StatusCode::OK, String::from_utf8_lossy(&bytes).into_owned());
            }
            Err(e) => tracing::warn!("{}/index.html을 읽지 못했어요: {e}", dir.display()),
        }
    }
    html_response(StatusCode::OK, BUILTIN_PAGE.to_owned())
}

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let h = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(h, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// URL 경로를 얼굴 폴더 안의 파일 경로로 바꾼다. 폴더 밖·숨김 파일·이상한 조각은 `None`.
pub fn safe_path(root: &Path, url_path: &str) -> Option<PathBuf> {
    let decoded = percent_decode(url_path.trim_start_matches('/'))?;
    if decoded.is_empty() {
        return None;
    }
    let mut p = root.to_path_buf();
    for seg in decoded.split('/') {
        let bad = seg.is_empty()
            || seg.starts_with('.')
            || seg.contains(['\\', ':', '\0', '*', '?', '"', '<', '>', '|']);
        if bad {
            return None;
        }
        p.push(seg);
    }
    Some(p)
}

fn content_type(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "webmanifest" => "application/manifest+json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// 얼굴 폴더의 정적 파일 하나(인증 없음, 비밀 없음). 맨 위 `index.html`은 `no-store`를 붙이는
/// `GET /`·`GET /index.html`로만 보낸다(인코딩한 경로로는 여기서 404).
pub async fn static_response(face_dir: Option<&Path>, url_path: &str) -> Response {
    let not_found = || {
        let mut r = (StatusCode::NOT_FOUND, "없는 파일이에요").into_response();
        secure_headers(r.headers_mut(), false);
        r
    };
    let Some(root) = face_dir else {
        return not_found();
    };
    let Some(path) = safe_path(root, url_path) else {
        return not_found();
    };
    if path.file_name().is_some_and(|n| n == "index.html") && path.parent() == Some(root) {
        return not_found();
    }
    let Ok(meta) = tokio::fs::metadata(&path).await else {
        return not_found();
    };
    if !meta.is_file() {
        return not_found();
    }
    let Ok(bytes) = tokio::fs::read(&path).await else {
        return not_found();
    };
    let immutable = url_path.trim_start_matches('/').starts_with("assets/");
    let mut r = Response::new(Body::from(bytes));
    let h = r.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(content_type(&path)),
    );
    secure_headers(h, false);
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(if immutable {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        }),
    );
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn builtin_page_has_secure_headers_and_no_secret() {
        let r = index_response(None).await;
        assert_eq!(r.status(), StatusCode::OK);
        let h = r.headers();
        assert_eq!(h[header::CONTENT_SECURITY_POLICY], CSP);
        assert_eq!(h[header::X_FRAME_OPTIONS], "DENY");
        assert_eq!(h[header::CACHE_CONTROL], "no-store");
        assert!(h.get(header::SET_COOKIE).is_none());
    }

    #[test]
    fn safe_paths() {
        let root = Path::new("/face");
        assert_eq!(
            safe_path(root, "/assets/app-1.js"),
            Some(PathBuf::from("/face/assets/app-1.js"))
        );
        assert_eq!(
            safe_path(root, "/assets/%ED%95%9C.png"),
            Some(PathBuf::from("/face/assets/한.png"))
        );
        for bad in [
            "/../etc/passwd",
            "/assets/../../x",
            "/assets/%2e%2e/x",
            "/.env",
            "/assets//x",
            "/a\\b",
            "/C:/x",
            "/",
            "/%zz",
            "/%00",
        ] {
            assert_eq!(safe_path(root, bad), None, "{bad}");
        }
    }

    #[test]
    fn content_types() {
        assert_eq!(
            content_type(Path::new("a.JS")),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(content_type(Path::new("a.woff2")), "font/woff2");
        assert_eq!(content_type(Path::new("a")), "application/octet-stream");
    }
}
