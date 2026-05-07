use embassy_futures::select::Either;
use embassy_time::{Duration, Timer};

pub async fn sleep(ms: u64) {
    Timer::after(Duration::from_millis(ms)).await;
}

pub fn either_into_result<T, E>(either: Either<Result<T, E>, Result<T, E>>) -> Result<T, E> {
    match either {
        Either::First(r) => r,
        Either::Second(r) => r,
    }
}
