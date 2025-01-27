use std::borrow::BorrowMut;
use std::cell::RefCell;
use std::cmp::Ord;
use std::cmp::Ordering;

use openssl::bn::{BigNum, BigNumContext, BigNumContextRef, BigNumRef, MsbOption};
use openssl::error::ErrorStack;

#[cfg(feature = "serde")]
use crate::serializable_crypto_primitive;
#[cfg(feature = "serde")]
use crate::serialization::{
    deserialize_crypto_primitive, serialize_crypto_primitive, SerializableCryptoPrimitive,
};
#[cfg(feature = "serde")]
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::{Error as ClError, Result as ClResult};

thread_local! {
    static BN_CONTEXT: RefCell<BigNumContext> = RefCell::new(BigNumContext::new_secure().unwrap());
}

fn with_bn_context<F, R>(f: F) -> R
where
    F: FnOnce(&mut BigNumContextRef) -> R,
{
    BN_CONTEXT.with(|cell| f(cell.borrow_mut().borrow_mut()))
}

#[derive(Debug)]
pub struct BigNumber {
    openssl_bn: BigNum,
}

impl BigNumber {
    pub fn new() -> ClResult<BigNumber> {
        let bn = BigNum::new_secure()?;
        Ok(BigNumber { openssl_bn: bn })
    }

    pub fn generate_prime(size: usize) -> ClResult<BigNumber> {
        let mut bn = BigNumber::new()?;
        BigNumRef::generate_prime(&mut bn.openssl_bn, size as i32, false, None, None)?;
        Ok(bn)
    }

    pub fn generate_safe_prime(size: usize) -> ClResult<BigNumber> {
        let mut bn = BigNumber::new()?;
        BigNumRef::generate_prime(&mut bn.openssl_bn, (size + 1) as i32, true, None, None)?;
        Ok(bn)
    }

    pub fn is_prime(&self) -> ClResult<bool> {
        let prime_len = self.openssl_bn.num_bits() as f32 * core::f32::consts::LOG10_2;
        let checks = prime_len.log2() as i32;
        Ok(with_bn_context(|ctx| {
            self.openssl_bn.is_prime_fasttest(checks, ctx, true)
        })?)
    }

    pub fn is_safe_prime(&self) -> ClResult<bool> {
        // according to https://eprint.iacr.org/2003/186.pdf
        // a safe prime is congruent to 2 mod 3

        // a safe prime satisfies (p-1)/2 is prime. Since a
        // prime is odd, We just need to divide by 2
        Ok(
            self.modulus(&BigNumber::from_u32(3)?)? == BigNumber::from_u32(2)?
                && self.is_prime()?
                && self.rshift1()?.is_prime()?,
        )
    }

    pub fn rand(size: usize) -> ClResult<BigNumber> {
        let mut bn = BigNumber::new()?;
        BigNumRef::rand(
            &mut bn.openssl_bn,
            size as i32,
            MsbOption::MAYBE_ZERO,
            false,
        )?;
        Ok(bn)
    }

    pub fn rand_range(&self) -> ClResult<BigNumber> {
        let mut bn = BigNumber::new()?;
        BigNumRef::rand_range(&self.openssl_bn, &mut bn.openssl_bn)?;
        Ok(bn)
    }

    pub fn num_bits(&self) -> ClResult<i32> {
        Ok(self.openssl_bn.num_bits())
    }

    pub fn is_bit_set(&self, n: i32) -> ClResult<bool> {
        Ok(self.openssl_bn.is_bit_set(n))
    }

    pub fn set_bit(&mut self, n: i32) -> ClResult<&mut BigNumber> {
        BigNumRef::set_bit(&mut self.openssl_bn, n)?;
        Ok(self)
    }

    pub fn from_u32(n: usize) -> ClResult<BigNumber> {
        let bn = BigNum::from_u32(n as u32)?;
        Ok(BigNumber { openssl_bn: bn })
    }

    pub fn from_dec(dec: &str) -> ClResult<BigNumber> {
        let bn = BigNum::from_dec_str(dec)?;
        Ok(BigNumber { openssl_bn: bn })
    }

    pub fn from_hex(hex: &str) -> ClResult<BigNumber> {
        let bn = BigNum::from_hex_str(hex)?;
        Ok(BigNumber { openssl_bn: bn })
    }

    pub fn from_bytes(bytes: &[u8]) -> ClResult<BigNumber> {
        let bn = BigNum::from_slice(bytes)?;
        Ok(BigNumber { openssl_bn: bn })
    }

    pub fn to_dec(&self) -> ClResult<String> {
        let result = self.openssl_bn.to_dec_str()?;
        Ok(result.to_string())
    }

    pub fn to_hex(&self) -> ClResult<String> {
        let result = self.openssl_bn.to_hex_str()?;
        Ok(result.to_string())
    }

    pub fn to_bytes(&self) -> ClResult<Vec<u8>> {
        Ok(self.openssl_bn.to_vec())
    }

    pub fn add(&self, a: &BigNumber) -> ClResult<BigNumber> {
        let mut bn = BigNumber::new()?;
        BigNumRef::checked_add(&mut bn.openssl_bn, &self.openssl_bn, &a.openssl_bn)?;
        Ok(bn)
    }

    pub fn sub(&self, a: &BigNumber) -> ClResult<BigNumber> {
        let mut bn = BigNumber::new()?;
        BigNumRef::checked_sub(&mut bn.openssl_bn, &self.openssl_bn, &a.openssl_bn)?;
        Ok(bn)
    }

    // TODO: There should be a mod_sqr using underlying math library's square modulo since squaring is faster.
    pub fn sqr(&self) -> ClResult<BigNumber> {
        let mut bn = BigNumber::new()?;
        with_bn_context(|ctx| BigNumRef::sqr(&mut bn.openssl_bn, &self.openssl_bn, ctx))?;
        Ok(bn)
    }

    pub fn mul(&self, a: &BigNumber) -> ClResult<BigNumber> {
        let mut bn = BigNumber::new()?;
        with_bn_context(|ctx| {
            BigNumRef::checked_mul(&mut bn.openssl_bn, &self.openssl_bn, &a.openssl_bn, ctx)
        })?;
        Ok(bn)
    }

    pub fn mod_mul(&self, a: &BigNumber, n: &BigNumber) -> ClResult<BigNumber> {
        let mut bn = BigNumber::new()?;
        with_bn_context(|ctx| {
            BigNumRef::mod_mul(
                &mut bn.openssl_bn,
                &self.openssl_bn,
                &a.openssl_bn,
                &n.openssl_bn,
                ctx,
            )
        })?;
        Ok(bn)
    }

    pub fn mod_sub(&self, a: &BigNumber, n: &BigNumber) -> ClResult<BigNumber> {
        let mut bn = BigNumber::new()?;
        with_bn_context(|ctx| {
            BigNumRef::mod_sub(
                &mut bn.openssl_bn,
                &self.openssl_bn,
                &a.openssl_bn,
                &n.openssl_bn,
                ctx,
            )
        })?;
        Ok(bn)
    }

    pub fn div(&self, a: &BigNumber) -> ClResult<BigNumber> {
        let mut bn = BigNumber::new()?;
        with_bn_context(|ctx| {
            BigNumRef::checked_div(&mut bn.openssl_bn, &self.openssl_bn, &a.openssl_bn, ctx)
        })?;
        Ok(bn)
    }

    pub fn gcd(a: &BigNumber, b: &BigNumber) -> ClResult<BigNumber> {
        let mut gcd = BigNumber::new()?;
        with_bn_context(|ctx| {
            BigNumRef::gcd(&mut gcd.openssl_bn, &a.openssl_bn, &b.openssl_bn, ctx)
        })?;
        Ok(gcd)
    }

    // Question: The *_word APIs seem odd. When the method is already mutating, why return the reference?

    pub fn add_word(&mut self, w: u32) -> ClResult<&mut BigNumber> {
        BigNumRef::add_word(&mut self.openssl_bn, w)?;
        Ok(self)
    }

    pub fn sub_word(&mut self, w: u32) -> ClResult<&mut BigNumber> {
        BigNumRef::sub_word(&mut self.openssl_bn, w)?;
        Ok(self)
    }

    pub fn mul_word(&mut self, w: u32) -> ClResult<&mut BigNumber> {
        BigNumRef::mul_word(&mut self.openssl_bn, w)?;
        Ok(self)
    }

    pub fn div_word(&mut self, w: u32) -> ClResult<&mut BigNumber> {
        BigNumRef::div_word(&mut self.openssl_bn, w)?;
        Ok(self)
    }

    pub fn mod_exp(&self, a: &BigNumber, b: &BigNumber) -> ClResult<BigNumber> {
        let mut bn = BigNumber::new()?;

        if a.openssl_bn.is_negative() {
            let (base, a1) = (self.inverse(b)?, a.set_negative(false)?);
            with_bn_context(|ctx| {
                BigNumRef::mod_exp(
                    &mut bn.openssl_bn,
                    &base.openssl_bn,
                    &a1.openssl_bn,
                    &b.openssl_bn,
                    ctx,
                )
            })?;
        } else {
            with_bn_context(|ctx| {
                BigNumRef::mod_exp(
                    &mut bn.openssl_bn,
                    &self.openssl_bn,
                    &a.openssl_bn,
                    &b.openssl_bn,
                    ctx,
                )
            })?;
        };
        Ok(bn)
    }

    pub fn modulus(&self, a: &BigNumber) -> ClResult<BigNumber> {
        let mut bn = BigNumber::new()?;
        with_bn_context(|ctx| {
            BigNumRef::nnmod(&mut bn.openssl_bn, &self.openssl_bn, &a.openssl_bn, ctx)
        })?;
        Ok(bn)
    }

    pub fn exp(&self, a: &BigNumber) -> ClResult<BigNumber> {
        let mut bn = BigNumber::new()?;
        with_bn_context(|ctx| {
            BigNumRef::exp(&mut bn.openssl_bn, &self.openssl_bn, &a.openssl_bn, ctx)
        })?;
        Ok(bn)
    }

    pub fn inverse(&self, n: &BigNumber) -> ClResult<BigNumber> {
        let mut bn = BigNumber::new()?;
        with_bn_context(|ctx| {
            BigNumRef::mod_inverse(&mut bn.openssl_bn, &self.openssl_bn, &n.openssl_bn, ctx)
        })?;
        Ok(bn)
    }

    pub fn set_negative(&self, negative: bool) -> ClResult<BigNumber> {
        let mut bn = BigNum::from_slice(&self.openssl_bn.to_vec())?;
        bn.set_negative(negative);
        Ok(BigNumber { openssl_bn: bn })
    }

    pub fn is_negative(&self) -> bool {
        self.openssl_bn.is_negative()
    }

    pub fn increment(&self) -> ClResult<BigNumber> {
        let mut bn = BigNum::from_slice(&self.openssl_bn.to_vec())?;
        bn.add_word(1)?;
        Ok(BigNumber { openssl_bn: bn })
    }

    pub fn decrement(&self) -> ClResult<BigNumber> {
        let mut bn = BigNum::from_slice(&self.openssl_bn.to_vec())?;
        bn.sub_word(1)?;
        Ok(BigNumber { openssl_bn: bn })
    }

    pub fn lshift1(&self) -> ClResult<BigNumber> {
        let mut bn = BigNumber::new()?;
        BigNumRef::lshift1(&mut bn.openssl_bn, &self.openssl_bn)?;
        Ok(bn)
    }

    pub fn rshift1(&self) -> ClResult<BigNumber> {
        let mut bn = BigNumber::new()?;
        BigNumRef::rshift1(&mut bn.openssl_bn, &self.openssl_bn)?;
        Ok(bn)
    }

    pub fn rshift(&self, n: u32) -> ClResult<BigNumber> {
        let mut bn = BigNumber::new()?;
        BigNumRef::rshift(&mut bn.openssl_bn, &self.openssl_bn, n as i32)?;
        Ok(bn)
    }

    ///(a * (1/b mod p) mod p)
    pub fn mod_div(&self, b: &BigNumber, p: &BigNumber) -> ClResult<BigNumber> {
        let mut bn = BigNumber::new()?;
        let b1 = &b.inverse(p)?;
        with_bn_context(|ctx| {
            BigNumRef::mod_mul(
                &mut bn.openssl_bn,
                &self.openssl_bn,
                &b1.openssl_bn,
                &p.openssl_bn,
                ctx,
            )
        })?;
        Ok(bn)
    }

    // Question: Why does this need to be a Result? When is creating a BigNumber same as another
    // BigNumber not possible given sufficient memory?
    pub fn try_clone(&self) -> ClResult<BigNumber> {
        let mut openssl_bn = BigNum::from_slice(&self.openssl_bn.to_vec()[..])?;
        openssl_bn.set_negative(self.is_negative());
        Ok(BigNumber { openssl_bn })
    }
}

impl Ord for BigNumber {
    fn cmp(&self, other: &BigNumber) -> Ordering {
        self.openssl_bn.cmp(&other.openssl_bn)
    }
}

impl Eq for BigNumber {}

impl PartialOrd for BigNumber {
    fn partial_cmp(&self, other: &BigNumber) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for BigNumber {
    fn eq(&self, other: &BigNumber) -> bool {
        self.openssl_bn == other.openssl_bn
    }
}

#[cfg(feature = "serde")]
impl SerializableCryptoPrimitive for BigNumber {
    fn name() -> &'static str {
        "BigNumber"
    }

    fn to_string(&self) -> ClResult<String> {
        self.to_dec()
    }

    fn to_bytes(&self) -> ClResult<Vec<u8>> {
        self.to_bytes()
    }

    fn from_string(value: &str) -> ClResult<Self> {
        BigNumber::from_dec(value)
    }

    fn from_bytes(value: &[u8]) -> ClResult<Self> {
        BigNumber::from_bytes(value)
    }
}

#[cfg(feature = "serde")]
serializable_crypto_primitive!(BigNumber);

impl From<ErrorStack> for ClError {
    fn from(err: ErrorStack) -> Self {
        // TODO: FIXME: Analyze ErrorStack and split invalid structure errors from other errors
        err_msg!(InvalidState, "Internal OpenSSL error: {}", err)
    }
}

impl Default for BigNumber {
    fn default() -> BigNumber {
        BigNumber::from_u32(0).unwrap()
    }
}
