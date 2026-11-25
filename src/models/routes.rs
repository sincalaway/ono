use crate::Ctx;
use reqwest::{
    blocking::{Client, Response},
    header::LOCATION,
    StatusCode, Url,
};
use serde_json::Value;
use std::time::Duration;

const HF: &str = "https://huggingface.co";

struct Route {
    endpoint: Url,
    client: Client,
    token: String,
}

fn field<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}

pub(super) fn validate_path(path: &str) -> Result<(), String> {
    if path.split('/').any(|s| {
        s.is_empty()
            || s == "."
            || s == ".."
            || s.contains(['\\', ':'])
            || s.chars().any(char::is_control)
    }) {
        return Err("仓库文件路径无效".into());
    }
    Ok(())
}

pub(super) fn strong_etag(r: &Response) -> Option<&str> {
    r.headers()
        .get("etag")?
        .to_str()
        .ok()
        .filter(|s| s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
}
impl Route {
    fn new(endpoint: &str, token: &str) -> Result<Self, String> {
        let endpoint = Url::parse(endpoint).map_err(|_| "镜像地址无效")?;
        if !matches!(endpoint.scheme(), "http" | "https")
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            return Err("镜像地址必须是 HTTP 或 HTTPS 基地址".into());
        }
        let client = Client::builder()
            .user_agent("ono/3.1.0")
            .connect_timeout(Duration::from_secs(12))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            endpoint,
            client,
            token: token.into(),
        })
    }

    fn url(&self, api: bool, parts: &[&str]) -> Result<Url, String> {
        let mut url = self.endpoint.clone();
        let mut segments = url.path_segments_mut().unwrap();
        segments.pop_if_empty();
        if api {
            segments.push("api");
        }
        for part in parts {
            validate_path(part)?;
            for s in part.split('/') {
                segments.push(s);
            }
        }
        drop(segments);
        Ok(url)
    }

    fn send(
        &self,
        mut url: Url,
        api: bool,
        range: Option<(u64, Option<&str>)>,
    ) -> Result<Response, String> {
        let mut authenticated = !self.token.is_empty();
        for _ in 0..10 {
            let mut request = self.client.get(url.clone());
            if api {
                request = request.timeout(Duration::from_secs(35));
            }
            if let Some((offset, validator)) = range {
                request = request.header("Range", format!("bytes={offset}-"));
                if let Some(tag) = validator {
                    request = request.header("If-Range", tag);
                }
            }
            if authenticated {
                request = request.bearer_auth(&self.token);
            }
            let response = request.send().map_err(|e| e.without_url().to_string())?;
            if !response.status().is_redirection() {
                return Ok(response);
            }
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|s| s.to_str().ok())
                .ok_or("下载重定向缺少地址")?;
            let target = url.join(location).map_err(|_| "下载重定向地址无效")?;
            if !matches!(target.scheme(), "https" | "http")
                || (url.scheme() == "https" && target.scheme() != "https")
                || !target.username().is_empty()
                || target.password().is_some()
            {
                return Err("下载重定向地址无效".into());
            }
            authenticated &= target.origin() == url.origin();
            url = target;
        }
        Err("下载重定向次数过多".into())
    }
}

pub(crate) struct Routes {
    route: Route,
}

impl Routes {
    pub(crate) fn new(ctx: &Ctx) -> Result<Self, String> {
        Self::from_settings(&ctx.settings()?)
    }

    fn from_settings(settings: &Value) -> Result<Self, String> {
        let mirror = field(settings, "custom_mirror").trim_end_matches('/');
        let public = if mirror.is_empty() { HF } else { mirror };
        let token = if public == HF {
            field(settings, "hf_token")
        } else {
            ""
        };
        Ok(Self {
            route: Route::new(public, token)?,
        })
    }

    pub(crate) fn fetch(
        &self,
        api: bool,
        parts: &[&str],
        query: &[(&str, &str)],
        range: Option<(u64, Option<&str>)>,
    ) -> Result<(Response, &'static str), String> {
        let route = &self.route;
        let mut url = route.url(api, parts)?;
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query);
        }
        let mut range = range;
        let mut response = route.send(url.clone(), api, range)?;
        if response.status() == StatusCode::PARTIAL_CONTENT
            && range.is_some_and(|(_, tag)| tag.is_some() && tag != strong_etag(&response))
        {
            range = None;
            response = route.send(url, api, None)?;
        }
        let status = response.status();
        if status == StatusCode::PARTIAL_CONTENT && range.is_none() {
            return Err("服务器返回了未请求的分段".into());
        }
        if status.is_success() || (range.is_some() && status == StatusCode::RANGE_NOT_SATISFIABLE)
        {
            return Ok((
                response,
                if route.endpoint.origin().ascii_serialization() == HF {
                    "官方源"
                } else {
                    "镜像源"
                },
            ));
        }
        Err(format!("模型服务器返回 HTTP {status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        thread,
    };

    fn server(reply: String) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        (
            origin,
            thread::spawn(move || {
                let (mut conn, _) = listener.accept().unwrap();
                let request = BufReader::new(&conn)
                    .lines()
                    .map(Result::unwrap)
                    .take_while(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n");
                conn.write_all(reply.as_bytes()).unwrap();
                request.to_ascii_lowercase()
            }),
        )
    }

    #[test]
    fn custom_mirror_is_used_without_hf_credentials() {
        let (public, public_request) =
            server("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n[]".into());
        let settings = serde_json::json!({"custom_mirror": public, "hf_token":"fixture-hf"});
        let routes = Routes::from_settings(&settings).unwrap();
        let (r, label) = routes
            .fetch(true, &["models", "a/b"], &[("blobs", "true")], None)
            .unwrap();
        assert_eq!(r.text().unwrap(), "[]");
        assert_eq!(label, "镜像源");
        let public = public_request.join().unwrap();
        assert!(public.starts_with("get /api/models/a/b?blobs=true "));
        assert!(!public.contains("fixture-hf"));
    }

    #[test]
    fn changed_etag_restarts_without_range() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let worker = thread::spawn(move || {
            for n in 0..2 {
                let (mut conn, _) = listener.accept().unwrap();
                let headers = BufReader::new(&conn)
                    .lines()
                    .map(Result::unwrap)
                    .take_while(|l| !l.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n")
                    .to_ascii_lowercase();
                if n == 0 {
                    assert!(headers.contains("range: bytes=2-"));
                    conn.write_all(b"HTTP/1.1 206 Partial Content\r\nETag: \"changed\"\r\nContent-Length: 1\r\nConnection: close\r\n\r\nx").unwrap();
                } else {
                    assert!(!headers.contains("range:"));
                    conn.write_all(b"HTTP/1.1 200 OK\r\nETag: \"changed\"\r\nContent-Length: 3\r\nConnection: close\r\n\r\nNEW").unwrap();
                }
            }
        });
        let routes = Routes::from_settings(&serde_json::json!({"custom_mirror": origin})).unwrap();
        let (r, _) = routes
            .fetch(
                false,
                &["a/b", "resolve", "sha", "f"],
                &[],
                Some((2, Some("\"old\""))),
            )
            .unwrap();
        assert_eq!(r.text().unwrap(), "NEW");
        worker.join().unwrap();
    }
}
