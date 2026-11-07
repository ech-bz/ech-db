use std::cell::RefCell;
use std::collections::BTreeMap;
use std::future::Future;
use std::time::{Duration, Instant};

tokio::task_local! {
    static CURRENT: RefCell<Timings>;
}

#[derive(Default)]
pub(crate) struct Timings {
    stages: BTreeMap<&'static str, (Duration, usize)>,
    details: Vec<String>,
    detail_bytes: usize,
    truncated: bool,
    guest: BTreeMap<String, (Duration, u64, usize)>,
}

impl Timings {
    pub(crate) fn header(self) -> String {
        let mut header = self
            .stages
            .into_iter()
            .map(|(name, (duration, count))| {
                format!(
                    "{name};dur={:.3};desc=\"calls={count}\"",
                    duration.as_secs_f64() * 1000.0
                )
            })
            .chain(
                self.guest
                    .into_iter()
                    .map(|(name, (duration, fuel, calls))| {
                        format!(
                            "db_guest_{name};dur={:.3};desc=\"calls={calls} fuel={fuel}\"",
                            duration.as_secs_f64() * 1000.0
                        )
                    }),
            )
            .collect::<Vec<_>>()
            .join(", ");
        let mut truncated = self.truncated;
        for detail in self.details {
            if header.len() + detail.len() + 32 <= 4096 {
                header.push_str(", ");
                header.push_str(&detail);
            } else {
                truncated = true;
            }
        }
        if truncated {
            header.push_str(", db_details_truncated");
        }
        header
    }
}

pub(crate) fn guest(name: String, duration: Duration, fuel: u64) {
    let _ = CURRENT.try_with(|current| {
        let mut current = current.borrow_mut();
        if current.guest.len() >= 16 && !current.guest.contains_key(&name) {
            current.truncated = true;
            return;
        }
        let entry = current.guest.entry(name).or_default();
        entry.0 += duration;
        entry.1 = entry.1.saturating_add(fuel);
        entry.2 += 1;
    });
}

pub(crate) async fn collect<T>(future: impl Future<Output = T>) -> (T, Timings) {
    CURRENT
        .scope(RefCell::new(Timings::default()), async {
            let value = future.await;
            let timings = CURRENT.with(|current| current.take());
            (value, timings)
        })
        .await
}

pub(crate) struct Timer {
    name: &'static str,
    started: Instant,
    finished: bool,
}

impl Timer {
    pub(crate) fn new(name: &'static str) -> Self {
        Self {
            name,
            started: Instant::now(),
            finished: false,
        }
    }

    pub(crate) fn finish(mut self, description: String) {
        let duration = self.started.elapsed();
        self.finished = true;
        self.record(duration, Some(description));
    }

    fn record(&self, duration: Duration, description: Option<String>) {
        let _ = CURRENT.try_with(|current| {
            let mut current = current.borrow_mut();
            let entry = current.stages.entry(self.name).or_default();
            entry.0 += duration;
            entry.1 += 1;
            let index = entry.1;
            if let Some(description) = description {
                let detail = format!(
                    "{}{};dur={:.3};desc=\"{}\"",
                    self.name,
                    index,
                    duration.as_secs_f64() * 1000.0,
                    description,
                );
                if current.details.len() < 16 && current.detail_bytes + detail.len() <= 1800 {
                    current.detail_bytes += detail.len();
                    current.details.push(detail);
                } else {
                    current.truncated = true;
                }
            }
        });
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        if !self.finished {
            self.record(self.started.elapsed(), None);
        }
    }
}

pub(crate) async fn measure_async<T>(name: &'static str, future: impl Future<Output = T>) -> T {
    let _timer = Timer::new(name);
    future.await
}

pub(crate) async fn middleware(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let (mut response, timings) = collect(measure_async("db", next.run(request))).await;
    if let Ok(value) = axum::http::HeaderValue::from_str(&timings.header()) {
        response.headers_mut().insert("server-timing", value);
    }
    response
}
