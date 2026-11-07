//! Ordered source preparation feeding bounded parallel drawing queues.

use std::sync::atomic::{AtomicBool, Ordering};

/// Prepare sources in timeline order while independent painters draw them.
/// Dropping output receivers on cancellation/error unblocks drawing workers,
/// which drop input receivers and unblock the producer before the scope joins.
pub(super) fn draw_frames<T: Send, P: Send, W: Send>(
    painters: Vec<T>,
    frames: u64,
    cancel: &AtomicBool,
    mut prepare: impl FnMut(u64) -> P + Send,
    draw: impl Fn(u64, P, &mut T) -> W + Sync,
    mut hand_on: impl FnMut(u64, W) -> Result<(), String>,
) -> Result<bool, String> {
    let count = painters.len();
    if count == 0 {
        return Err("no drawing workers were available".into());
    }
    std::thread::scope(|scope| {
        let mut drawn = Vec::with_capacity(count);
        let mut inputs = Vec::with_capacity(count);
        for (i, mut painter) in painters.into_iter().enumerate() {
            let draw = &draw;
            let (input, ready) = std::sync::mpsc::sync_channel::<(u64, P)>(1);
            let (tx, rx) = std::sync::mpsc::sync_channel::<W>(2);
            inputs.push(input);
            drawn.push(rx);
            std::thread::Builder::new()
                .name(format!("bv-render-{i}"))
                .stack_size(bv_ofx::PLUGIN_STACK)
                .spawn_scoped(scope, move || {
                    for (n, prepared) in ready {
                        if cancel.load(Ordering::Relaxed)
                            || tx.send(draw(n, prepared, &mut painter)).is_err()
                        {
                            break;
                        }
                    }
                })
                .map_err(|e| e.to_string())?;
        }
        std::thread::Builder::new()
            .name("noni-export-decode".into())
            .spawn_scoped(scope, move || {
                for n in 0..frames {
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    if inputs[(n % count as u64) as usize]
                        .send((n, prepare(n)))
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        for n in 0..frames {
            if cancel.load(Ordering::Relaxed) {
                return Ok(true);
            }
            let frame = drawn[(n % count as u64) as usize]
                .recv()
                .map_err(|_| "a drawing thread stopped".to_string())?;
            hand_on(n, frame)?;
        }
        Ok(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preparation_is_once_in_order_and_drawing_is_handed_on_in_order() {
        let cancel = AtomicBool::new(false);
        let prepared = std::sync::Mutex::new(Vec::new());
        let mut output = Vec::new();
        let cancelled = draw_frames(
            vec![0, 1, 2],
            101,
            &cancel,
            |n| {
                prepared.lock().unwrap().push(n);
                n * 3
            },
            |n, picture, worker| {
                assert_eq!(*worker, (n % 3) as usize);
                if *worker == 0 {
                    std::thread::yield_now();
                }
                picture
            },
            |n, picture| {
                output.push((n, picture));
                Ok(())
            },
        )
        .unwrap();
        assert!(!cancelled);
        assert_eq!(*prepared.lock().unwrap(), (0..101).collect::<Vec<_>>());
        assert_eq!(output, (0..101).map(|n| (n, n * 3)).collect::<Vec<_>>());
    }

    #[test]
    fn cancellation_releases_full_input_and_output_queues() {
        let cancel = AtomicBool::new(false);
        let result = draw_frames(
            vec![(), ()],
            10_000,
            &cancel,
            |n| n,
            |_, p, _| p,
            |n, _| {
                if n == 4 {
                    cancel.store(true, Ordering::Relaxed);
                }
                Ok(())
            },
        );
        assert_eq!(result, Ok(true));
    }

    #[test]
    fn writer_failure_releases_full_input_and_output_queues() {
        let cancel = AtomicBool::new(false);
        let result = draw_frames(
            vec![(), ()],
            10_000,
            &cancel,
            |n| n,
            |_, p, _| p,
            |_, _| Err("writer failed".into()),
        );
        assert_eq!(result, Err("writer failed".into()));
    }
}
