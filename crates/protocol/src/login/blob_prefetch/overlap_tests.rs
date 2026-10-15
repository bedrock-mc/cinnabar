use super::prefetch_while_reading;
use futures::task::noop_waker;
use std::{
    cell::Cell,
    future::{Future, poll_fn, ready},
    task::{Context, Poll},
};

#[test]
fn completed_prefetch_never_reads_another_frame() {
    let mut overlap = std::pin::pin!(prefetch_while_reading(ready(()), |_| {
        panic!("a completed disk read must win before raw intake")
    }));
    let waker = noop_waker();
    assert!(
        overlap
            .as_mut()
            .poll(&mut Context::from_waker(&waker))
            .is_ready()
    );
}

#[test]
fn hot_reader_rechecks_disk_completion_between_individual_frames() {
    let reads = Cell::new(0);
    let polls = Cell::new(0);
    let prefetch = poll_fn(|_| {
        polls.set(polls.get() + 1);
        if reads.get() == 3 {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    });
    let mut overlap = std::pin::pin!(prefetch_while_reading(prefetch, |_| {
        reads.set(reads.get() + 1);
        assert!(
            reads.get() <= 3,
            "ready disk completion must stop a hot reader"
        );
        Poll::Ready(true)
    }));
    let waker = noop_waker();
    assert!(
        overlap
            .as_mut()
            .poll(&mut Context::from_waker(&waker))
            .is_ready()
    );
    assert_eq!(reads.get(), 3);
    assert_eq!(polls.get(), 4);
}

#[test]
fn stopped_reader_keeps_waiting_on_the_original_prefetch() {
    let complete = Cell::new(false);
    let starts = Cell::new(0);
    let reads = Cell::new(0);
    let prefetch = async {
        starts.set(starts.get() + 1);
        poll_fn(|_| {
            if complete.get() {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
    };
    let mut overlap = std::pin::pin!(prefetch_while_reading(prefetch, |_| {
        reads.set(reads.get() + 1);
        Poll::Ready(false)
    }));
    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);
    assert!(overlap.as_mut().poll(&mut cx).is_pending());
    assert!(overlap.as_mut().poll(&mut cx).is_pending());
    assert_eq!(starts.get(), 1);
    assert_eq!(
        reads.get(),
        1,
        "bound or terminal must permanently stop this intake wait"
    );
    complete.set(true);
    assert!(overlap.as_mut().poll(&mut cx).is_ready());
    assert_eq!(starts.get(), 1);
    assert_eq!(reads.get(), 1);
}

#[test]
fn disk_completion_does_not_wait_for_new_socket_input() {
    let complete = Cell::new(false);
    let reads = Cell::new(0);
    let prefetch = poll_fn(|_| {
        if complete.get() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    });
    let mut overlap = std::pin::pin!(prefetch_while_reading(prefetch, |_| {
        reads.set(reads.get() + 1);
        Poll::Pending
    }));
    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);
    assert!(overlap.as_mut().poll(&mut cx).is_pending());
    assert_eq!(reads.get(), 1);
    complete.set(true);
    assert!(overlap.as_mut().poll(&mut cx).is_ready());
    assert_eq!(reads.get(), 1);
}
