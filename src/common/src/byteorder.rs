// This Source Code Form is subject to the terms of the Mozilla Public License,
// v. 2.0. If a copy of the MPL was not distributed with this file, You can
// obtain one at https://mozilla.org/MPL/2.0/.

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct BigEndianU16(u16);

impl BigEndianU16 {
    pub fn get(&self) -> u16 {
        #[cfg(target_endian = "little")]
        return self.0.swap_bytes();

        #[cfg(target_endian = "big")]
        return self.0;
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct BigEndianU32(u32);

impl BigEndianU32 {
    pub fn get(&self) -> u32 {
        #[cfg(target_endian = "little")]
        return self.0.swap_bytes();

        #[cfg(target_endian = "big")]
        return self.0;
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct BigEndianU64(u64);

impl BigEndianU64 {
    pub fn get(&self) -> u64 {
        #[cfg(target_endian = "little")]
        return self.0.swap_bytes();

        #[cfg(target_endian = "big")]
        return self.0;
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct BigEndianI16(i16);

impl BigEndianI16 {
    pub fn get(&self) -> i16 {
        #[cfg(target_endian = "little")]
        return self.0.swap_bytes();

        #[cfg(target_endian = "big")]
        return self.0;
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct BigEndianI32(i32);

impl BigEndianI32 {
    pub fn get(&self) -> i32 {
        #[cfg(target_endian = "little")]
        return self.0.swap_bytes();

        #[cfg(target_endian = "big")]
        return self.0;
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct BigEndianI64(i64);

impl BigEndianI64 {
    pub fn get(&self) -> i64 {
        #[cfg(target_endian = "little")]
        return self.0.swap_bytes();

        #[cfg(target_endian = "big")]
        return self.0;
    }
}

macro_rules! implDebug {
    ($($t:ty),+) => {
        $(
            impl core::fmt::Debug for $t {
                fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                    core::fmt::Debug::fmt(&self.get(), f)
                }
            }
        )+
    };
}

implDebug!(BigEndianU16, BigEndianU32, BigEndianU64, BigEndianI16, BigEndianI32, BigEndianI64);

pub trait FromBytes: Sized {
    const SIZE: usize;

    fn from_bytes(bytes: &[u8]) -> Option<Self>;
}

macro_rules! implFromBytes {
    ($wrapper:ident $t:ty, $($tts:tt)*) => {
        impl FromBytes for $wrapper {
            const SIZE: usize = core::mem::size_of::<Self>();

            fn from_bytes(bytes: &[u8]) -> Option<Self> {
                use core::convert::TryInto;

                Some($wrapper({
                    let array: [u8; core::mem::size_of::<Self>()] = bytes.get(..core::mem::size_of::<Self>())?.try_into().ok()?;
                    <$t>::from_ne_bytes(array)
                }))
            }
        }

        implFromBytes!($($tts)*);
    };
    ($t:ty, $($tts:tt)*) => {
        impl FromBytes for $t {
            const SIZE: usize = core::mem::size_of::<Self>();

            fn from_bytes(bytes: &[u8]) -> Option<Self> {
                use core::convert::TryInto;

                Some({
                    let array: [u8; core::mem::size_of::<Self>()] = bytes.get(..core::mem::size_of::<Self>())?.try_into().ok()?;
                    <$t>::from_ne_bytes(array)
                })
            }
        }

        implFromBytes!($($tts)*);
    };
    () => {};
}

implFromBytes!(u8, i8, u16, i16, u32, i32, u64, i64, u128, i128, BigEndianU16 u16, BigEndianU32 u32, BigEndianU64 u64, BigEndianI16 i16, BigEndianI32 i32, BigEndianI64 i64,);

pub trait SliceExt {
    fn at<I: FromBytes>(&self, index: usize) -> Option<I>;
}

impl SliceExt for [u8] {
    fn at<I: FromBytes>(&self, index: usize) -> Option<I> {
        I::from_bytes(self.get(index..)?)
    }
}

pub struct IntegerStream<'a> {
    bytes: &'a [u8],
}

impl<'a> IntegerStream<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next<I: FromBytes>(&mut self) -> Option<I> {
        let ret = I::from_bytes(self.bytes)?;
        self.bytes = &self.bytes[I::SIZE..];

        Some(ret)
    }

    pub fn skip_n<I: FromBytes>(&mut self, n: usize) {
        self.bytes = self.bytes.get(I::SIZE * n..).unwrap_or_default();
    }

    pub fn remaining(&self) -> &[u8] {
        self.bytes
    }
}

#[macro_export]
macro_rules! stream_ints {
    ($stream:ident, {
        $($tts:tt)*
    }) => {
        $crate::stream_ints!(@internal $stream $($tts)*);
    };

    (@internal $stream:ident skip $l:literal bytes, $($tts:tt)*) => {
        $stream.skip_n::<u8>($l);
        $crate::stream_ints!(@internal $stream $($tts)*);
    };

    (@internal $stream:ident let $name:ident: $t:ident, $($tts:tt)*) => {
        let $name: $t = $stream.next().expect(concat!(stringify!($name), " wasn't valid in the stream"));
        $crate::stream_ints!(@internal $stream $($tts)*);
    };

    (@internal $stream:ident let $name:ident: $t:ident?, $($tts:tt)*) => {
        let $name: $t = $stream.next()?;
        $crate::stream_ints!(@internal $stream $($tts)*);
    };

    (@internal $stream:ident) => {};
}
