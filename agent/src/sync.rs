//! Kilitler: bir thread kilit tutarken paniklerse std::Mutex "zehirlenir" ve ona dokunan her thread de çökerdi
//! (bir hata tüm programı düşürürdü). Burada zehirlenmiş kilit yok sayılıp içindeki veriyle devam edilir.
use std::sync::{Mutex, MutexGuard};

pub trait LockExt<T> {
    fn lk(&self) -> MutexGuard<'_, T>;
}

impl<T> LockExt<T> for Mutex<T> {
    fn lk(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|e| e.into_inner())
    }
}
