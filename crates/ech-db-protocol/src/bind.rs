use serde::Serialize;

pub trait Bind {
    fn bind<N: Serialize + ?Sized>(&mut self, name: &N);

    fn open<N: Serialize + ?Sized>(name: &N) -> Self
    where
        Self: Default,
    {
        let mut value = Self::default();
        value.bind(name);
        value
    }
}

macro_rules! bind_noop {
    ($($ty:ty),* $(,)?) => {
        $(impl Bind for $ty {
            fn bind<N: Serialize + ?Sized>(&mut self, _name: &N) {}
        })*
    };
}

bind_noop!(u8, u16, u32, u64, u128, i8, i16, i32, i64, bool, String);

impl<const LENGTH: usize> Bind for [u8; LENGTH] {
    fn bind<N: Serialize + ?Sized>(&mut self, _name: &N) {}
}

impl<T: Bind> Bind for Option<T> {
    fn bind<N: Serialize + ?Sized>(&mut self, name: &N) {
        if let Some(value) = self {
            value.bind(name);
        }
    }
}

impl<T: Bind> Bind for Vec<T> {
    fn bind<N: Serialize + ?Sized>(&mut self, name: &N) {
        for value in self {
            value.bind(name);
        }
    }
}

impl<A: Bind, B: Bind> Bind for (A, B) {
    fn bind<N: Serialize + ?Sized>(&mut self, name: &N) {
        self.0.bind(name);
        self.1.bind(name);
    }
}
