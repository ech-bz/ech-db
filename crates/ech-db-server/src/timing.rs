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
}

impl Timings {
    pub(crate) fn header(self) -> String {
        self.stages
            .into_iter()
            .map(|(name, (duration, count))| {
                format!(
                    "{name};dur={:.3};desc=\"calls={count}\"",
                    duration.as_secs_f64() * 1000.0
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
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
}

impl Timer {
    pub(crate) fn new(name: &'static str) -> Self {
        Self {
            name,
            started: Instant::now(),
        }
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        let duration = self.started.elapsed();
        let _ = CURRENT.try_with(|current| {
            let mut current = current.borrow_mut();
            let entry = current.stages.entry(self.name).or_default();
            entry.0 += duration;
            entry.1 += 1;
        });
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
