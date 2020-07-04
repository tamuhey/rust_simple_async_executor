use std::{
    future::Future,
    pin::Pin,
    sync::{
        mpsc::{self, Receiver, SyncSender},
        Arc, Mutex,
    },
    task::{Context, Poll, Waker},
    thread,
    time::Duration,
};

use futures::{
    future::{BoxFuture, FutureExt},
    task::{waker_ref, ArcWake},
};

// A future to simulate an asynchronous timer
struct TimerFutuer {
    state: Arc<Mutex<TimerState>>,
}

struct TimerState {
    // If the timer is completed or not
    is_completed: bool,
    // To call the executor to poll itself and tell it the timer has been completed.
    waker: Option<Waker>,
}

impl Future for TimerFutuer {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut state = self.state.lock().unwrap();
        if state.is_completed {
            Poll::Ready(())
        } else {
            state.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

impl TimerFutuer {
    fn new(dur: Duration) -> Self {
        let state = Arc::new(Mutex::new(TimerState {
            is_completed: false,
            waker: None,
        }));
        let shared_state = state.clone();
        thread::spawn(move || {
            thread::sleep(dur);
            let mut lock = shared_state.lock().unwrap();
            lock.is_completed = true;
            if let Some(waker) = lock.waker.take() {
                waker.wake();
            }
        });
        TimerFutuer { state }
    }
}

struct Executor {
    receiver: Receiver<Arc<Task>>,
}

impl Executor {
    fn run(&self) {
        while let Ok(task) = self.receiver.recv() {
            let waker = waker_ref(&task);
            let mut context = Context::from_waker(&waker);
            let mut future = task.future.lock().unwrap();
            if let Poll::Pending = future.poll_unpin(&mut context) {};
        }
    }
}

struct Spawner {
    sender: SyncSender<Arc<Task>>,
}

impl Spawner {
    fn spawn(&self, future: impl Future<Output = ()> + 'static + Send) {
        let future = Mutex::new(future.boxed());
        let sender = self.sender.clone();
        let task = Arc::new(Task { future, sender });
        self.sender.send(task).unwrap();
    }
}

struct Task {
    future: Mutex<BoxFuture<'static, ()>>,
    sender: SyncSender<Arc<Task>>,
}

impl ArcWake for Task {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        let task = arc_self.clone();
        arc_self.sender.send(task).unwrap();
    }
}

fn main() {
    let (sender, receiver) = mpsc::sync_channel::<Arc<Task>>(10_000);
    let executor = Executor { receiver };
    let spawner = Spawner { sender };

    async fn timer(dur: Duration) {
        println!("start timer {:?}", dur);
        let future = TimerFutuer::new(dur);
        future.await;
        println!("end   timer {:?}", dur);
    }

    spawner.spawn(timer(Duration::from_millis(3000)));
    spawner.spawn(timer(Duration::from_millis(1500)));

    // drop to remove all sender
    drop(spawner);
    executor.run();
}
