use super::libc::{
    pthread_mutex_t,
    timespec,
    pthread_mutex_trylock,
    EBUSY,
    ETIMEDOUT,
};

pub fn pthread_mutex_timedlock(lock: *mut pthread_mutex_t, abstime: &timespec) -> c_int {

    compile_error!("Must implement pthread_mutex_timedlock on macos");

    //call pthread_mutex_trylock until not EBUSY with some sort of sleep to prevent busy loop

	ETIMEDOUT
}
