use dotenv::dotenv;
use std::{env, sync::Arc};
use tokio::sync::{mpsc, Mutex, Barrier};
use indicatif::{ProgressBar, ProgressStyle};
use std::process::Command;
use rand::{seq::SliceRandom, thread_rng};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();

    let oxylabs_customer = env::var("OXYLABS_CUSTOMER").expect("OXYLABS_CUSTOMER");
    let oxylabs_password = env::var("OXYLABS_PASSWORD").expect("OXYLABS_PASSWORD");
    let oxylabs_url = env::var("OXYLABS_URL").expect("OXYLABS_URL");
    let oxylabs_port = env::var("OXYLABS_PORT").expect("OXYLABS_PORT");
    let max_concurrent_requests = env::var("MAX_CONCURRENT_REQUESTS")
        .unwrap()
        .parse::<usize>()?;
    let retries = env::var("MAX_RETRIES")
        .unwrap()
        .parse::<usize>()?;

    let proxy_url = format!("http://customer-{}:{}@{}:{}", oxylabs_customer, oxylabs_password, oxylabs_url, oxylabs_port);

    let client = Client::new(proxy_url);

    let listings = vec!["3314123","3318711","2672753","7097799ha","4705523ha","3136171","7146884ha", "9595134ha", "9828968ha", "303539", "10332801ha", "11087532ha", "8374868ha", "473337"];

    let total_listings = listings.len() as u64;
    let progress_bar = ProgressBar::new(total_listings * 5);
    progress_bar.set_style(ProgressStyle::default_bar()
        .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} (ETA: {eta}) | {msg}")?);

    let stats = Arc::new(Mutex::new(Statistics {
        successful_requests: 0,
        failed_requests: 0,
    }));

    let (tx, rx) = mpsc::channel(100);
    let rx = Arc::new(Mutex::new(rx));
    let progress_bar = Arc::new(progress_bar);

    let barrier = Arc::new(Barrier::new(max_concurrent_requests + 1));
    for id in 0..max_concurrent_requests {
        let barrier_clone = barrier.clone();
        let rx = Arc::clone(&rx);
        let client_clone = client.clone();
        let stats_clone = Arc::clone(&stats);
        let progress_bar_clone = Arc::clone(&progress_bar);
        tokio::spawn(async move {
            worker(id, client_clone, retries, rx, stats_clone, progress_bar_clone).await;
            barrier_clone.wait().await;
        });
    }

    for _ in 0..5 {
        let mut listings_clone = listings.clone();
        let mut rng = thread_rng();
        listings_clone.shuffle(&mut rng);

        for listing in listings_clone {
            tx.send(listing.to_string()).await.unwrap();
        }
    }

    drop(tx);

    barrier.wait().await;

    let stats = stats.lock().await;
    progress_bar.finish_with_message(format!("Success: {}, Failed: {}", stats.successful_requests, stats.failed_requests));

    Ok(())
}

async fn worker(_id: usize, client: Client, retries: usize, rx: Arc<Mutex<mpsc::Receiver<String>>>, stats: Arc<Mutex<Statistics>>, progress_bar: Arc<ProgressBar>) {
    loop {
        let url: String = {
            let mut rx = rx.lock().await;
            match rx.recv().await {
                Some(native_id) => format!("https://www.vrbo.com/{}", native_id),
                None => break,
            }
        };

        let result = download_page(&url, &client, retries).await;

        let mut stats = stats.lock().await;
        if let Ok(_content) = result {
            stats.successful_requests += 1;
        } else {
            progress_bar.println(format!("result: {:?}, url: {}", result, url));
            stats.failed_requests += 1;
        }
        progress_bar.set_message(format!("Success: {}, Failed: {}", stats.successful_requests, stats.failed_requests));
        progress_bar.inc(1);
    }
}

async fn download_page(url: &str, client: &Client, retries: usize) -> Result<String, DownloadError> {
    let max_attempts = retries + 1;
    let mut attempts = 0;

    loop {
        match client.get(url) {
            Ok(page) => return Ok(page),
            Err(e) => {
                if attempts >= max_attempts {
                    return Err(e);
                }
                attempts += 1;
            },
        }
    }
}

struct Statistics {
    successful_requests: u32,
    failed_requests: u32,
}

#[derive(Debug)]
enum DownloadError {
    UnexpectedStatusCode(String),
    CurlError,
    ProxyError,
}

#[derive(Clone)]
struct Client {
    proxy_url: String,
}

impl Client {
    fn new(proxy_url: String) -> Self {
        Self {
            proxy_url,
        }
    }

    fn get(&self, url: &str) -> Result<String, DownloadError> {
        let output = Command::new("curl-impersonate-chrome")
            .arg("-I")
            .arg(url)
            .args(&["--proxy", &self.proxy_url])
            .args(&["--ciphers", "TLS_AES_128_GCM_SHA256,TLS_AES_256_GCM_SHA384,TLS_CHACHA20_POLY1305_SHA256,ECDHE-ECDSA-AES128-GCM-SHA256,ECDHE-RSA-AES128-GCM-SHA256,ECDHE-ECDSA-AES256-GCM-SHA384,ECDHE-RSA-AES256-GCM-SHA384,ECDHE-ECDSA-CHACHA20-POLY1305,ECDHE-RSA-CHACHA20-POLY1305,ECDHE-RSA-AES128-SHA,ECDHE-RSA-AES256-SHA,AES128-GCM-SHA256,AES256-GCM-SHA384,AES128-SHA,AES256-SHA"])
            .args(&["-H", "sec-ch-ua: \"Chromium\";v=\"116\", \"Not)A;Brand\";v=\"24\", \"Google Chrome\";v=\"116\""])
            .args(&["-H", "sec-ch-ua-mobile: ?0"])
            .args(&["-H", "sec-ch-ua-platform: \"Windows\""])
            .args(&["-H", "Upgrade-Insecure-Requests: 1"])
            .args(&["-H", "User-Agent: Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/116.0.0.0 Safari/537.36"])
            .args(&["-H", "Accept: text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7"])
            .args(&["-H", "Sec-Fetch-Site: none"])
            .args(&["-H", "Sec-Fetch-Mode: navigate"])
            .args(&["-H", "Sec-Fetch-User: ?1"])
            .args(&["-H", "Sec-Fetch-Dest: document"])
            .args(&["-H", "Accept-Encoding: gzip, deflate, br"])
            .args(&["-H", "Accept-Language: en-US,en;q=0.9"])
            .args(&["--http2", "--http2-no-server-push", "--compressed"])
            .args(&["--tlsv1.2", "--alps", "--tls-permute-extensions"])
            .args(&["--cert-compression", "brotli"])
            .output()
            .expect("Failed to execute command");

        if !output.status.success() {
            return Err(DownloadError::CurlError);
        }

        let output_str = String::from_utf8_lossy(&output.stdout);
        let mut lines = output_str.lines();

        let proxy_status_code = {
            let mut status_code = None;

            while let Some(line) = lines.next() {
                if line.starts_with("HTTP/1.1 ") {
                    status_code = Some(line[9..12].parse::<u16>().unwrap_or(0));
                    break;
                }
            }

            match status_code {
                Some(status_code) => status_code,
                None => return Err(DownloadError::CurlError),
            }
        };

        if proxy_status_code != 200 {
            return Err(DownloadError::ProxyError);
        }

        let remote_status_code = {
            let mut status_code = None;

            while let Some(line) = lines.next() {
                if line.starts_with("HTTP/2 ") {
                    status_code = Some(line[7..10].parse::<u16>().unwrap_or(0));
                    break;
                }
            }

            match status_code {
                Some(status_code) => status_code,
                None => return Err(DownloadError::CurlError),
            }
        };

        if remote_status_code != 200 {
            return Err(DownloadError::UnexpectedStatusCode(format!("Remote status code: {}", remote_status_code)));
        }

        Ok(output_str.to_string())
    }
}