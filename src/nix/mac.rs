use super::libc::{
    pthread_mutex_t,
    timespec,
    pthread_mutex_trylock,
    EBUSY,
    ETIMEDOUT,
    nanosleep,
    c_int,
};

pub fn pthread_mutex_timedlock(lock: *mut pthread_mutex_t, abstime: &timespec) -> c_int {

    compile_error!("Must implement pthread_mutex_timedlock on macos");

    let mut timenow: timespec = timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };

    let timesleep: timespec = timespec {
        tv_sec: 0,
        tv_nsec: 10_000_000, // 10ms
    };

    let mut res: c_int;

    loop {
        res = unsafe {pthread_mutex_trylock(lock)};

        if res == EBUSY {
            // Check timeout before sleeping
            clock_gettime(CLOCK_REALTIME, &mut timenow)
            if timenow.tv_sec >= abstime.tv_sec && timenow.tv_nsec >= abstime->tv_nsec {
                return ETIMEDOUT;
            }

            //Sleep for a bit
            unsafe {nanosleep(&timesleep, null_mut())};

            continue;
        }

        break;
    }

    res
 }
