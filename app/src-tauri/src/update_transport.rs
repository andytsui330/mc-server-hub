use reqwest::{ClientBuilder, Url};
pub fn allowed(url: &Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && matches!(
            url.host_str(),
            Some("github.com" | "release-assets.githubusercontent.com" | "objects.githubusercontent.com" | "github-releases.githubusercontent.com")
        )
}
pub fn configure(client: ClientBuilder) -> ClientBuilder {
    client
        .https_only(true)
        .connect_timeout(std::time::Duration::from_secs(10))
        .read_timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 {
                attempt.error("更新轉址次數過多")
            } else if !allowed(attempt.url()) {
                attempt.error("更新轉址不在 HTTPS 官方網域白名單")
            } else {
                attempt.follow()
            }
        }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::{Duration, Instant},
    };
    fn init_crypto() {
        use tauri_plugin_updater::UpdaterExt;
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let mut context = tauri::test::mock_context(tauri::test::noop_assets());
            context.config_mut().plugins.0.insert(
                "updater".into(),
                serde_json::json!({"pubkey":"unused","dangerousInsecureTransportProtocol":true}),
            );
            let app = tauri::test::mock_builder()
                .plugin(tauri_plugin_updater::Builder::new().build())
                .build(context)
                .unwrap();
            let updater = app
                .updater_builder()
                .endpoints(vec![format!("http://{}", listener.local_addr().unwrap()).parse().unwrap()])
                .unwrap()
                .no_proxy()
                .timeout(Duration::from_millis(1))
                .build()
                .unwrap();
            let _ = tauri::async_runtime::block_on(updater.check());
        });
    }
    fn server(redirect: bool) -> (String, Arc<AtomicUsize>, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let seen = count.clone();
        let thread = std::thread::spawn(move || {
            let end = Instant::now() + Duration::from_millis(800);
            while Instant::now() < end {
                match listener.accept() {
                    Ok((mut s, _)) => {
                        s.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
                        let mut b = [0; 2048];
                        let _ = s.read(&mut b);
                        let n = seen.fetch_add(1, Ordering::SeqCst);
                        let response = if redirect && n == 0 {
                            format!("HTTP/1.1 302 Found\r\nLocation: http://{addr}/forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                        } else {
                            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".into()
                        };
                        let _ = s.write_all(response.as_bytes());
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(5)),
                    Err(_) => break,
                }
            }
        });
        (format!("http://{addr}/start"), count, thread)
    }
    #[test]
    fn only_exact_github_hosts_over_https() {
        for url in [
            "https://github.com/a",
            "https://release-assets.githubusercontent.com/a",
            "https://objects.githubusercontent.com/a",
        ] {
            assert!(allowed(&url.parse().unwrap()));
        }
        for url in [
            "http://github.com/a",
            "https://github.com.evil.invalid/a",
            "https://example.com/a",
            "https://u:p@github.com/a",
        ] {
            assert!(!allowed(&url.parse().unwrap()), "{url}");
        }
    }
    #[test]
    fn rejects_plain_http_before_connection() {
        init_crypto();
        let (url, count, t) = server(false);
        let result = tauri::async_runtime::block_on(async { configure(ClientBuilder::new()).no_proxy().build().unwrap().get(url).send().await });
        t.join().unwrap();
        assert!(result.is_err());
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }
    #[test]
    fn rejects_redirect_before_following_it() {
        init_crypto();
        let (url, count, t) = server(true);
        let result = tauri::async_runtime::block_on(async {
            configure(ClientBuilder::new())
                .https_only(false)
                .no_proxy()
                .build()
                .unwrap()
                .get(url)
                .send()
                .await
        });
        t.join().unwrap();
        assert!(result.unwrap_err().is_redirect());
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }
}
