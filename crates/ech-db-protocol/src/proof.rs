use curve25519_dalek::scalar::Scalar;
use serde::{Deserialize, Serialize};

use crate::crypto::{Curve, Hs};
use crate::error::Reject;

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Term {
    pub base: [u8; 32],
    pub response: u16,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Equation {
    pub terms: Vec<Term>,
    pub target: [u8; 32],
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Statement {
    pub equations: Vec<Equation>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Proof {
    pub commitment: Vec<[u8; 32]>,
    pub response: Vec<[u8; 32]>,
}

impl Proof {
    pub fn verify(
        &self,
        domain: &[u8],
        statement: &Statement,
        context: &[u8],
    ) -> Result<(), Reject> {
        if self.commitment.len() != statement.equations.len() {
            return Err(Reject::BadSignature);
        }
        let responses = statement
            .equations
            .iter()
            .flat_map(|equation| equation.terms.iter())
            .map(|term| term.response)
            .max()
            .map_or(0, |max| usize::from(max) + 1);
        if self.response.len() != responses {
            return Err(Reject::BadSignature);
        }
        let payload = bcs::to_bytes(&(statement, context, &self.commitment))
            .map_err(|_| Reject::BadRequest)?;
        let challenge = Hs::scalar(domain, &payload);
        for (equation, commitment) in statement.equations.iter().zip(&self.commitment) {
            let commitment = Curve::point(commitment).ok_or(Reject::BadSignature)?;
            let target = Curve::point(&equation.target).ok_or(Reject::BadSignature)?;
            let mut bases = Vec::with_capacity(equation.terms.len());
            let mut responses = Vec::with_capacity(equation.terms.len());
            for term in &equation.terms {
                let response = self
                    .response
                    .get(usize::from(term.response))
                    .ok_or(Reject::BadSignature)?;
                bases.push(Curve::point(&term.base).ok_or(Reject::BadSignature)?);
                responses.push(Curve::scalar(response).ok_or(Reject::BadSignature)?);
            }
            if !Curve::check(&bases, &responses, &commitment, &challenge, &target) {
                return Err(Reject::BadSignature);
            }
        }
        Ok(())
    }

    pub fn prove(
        domain: &[u8],
        statement: &Statement,
        context: &[u8],
        secrets: &[Scalar],
        nonces: &[Scalar],
    ) -> Option<Self> {
        if secrets.len() != nonces.len() {
            return None;
        }
        let mut commitment = Vec::with_capacity(statement.equations.len());
        for equation in &statement.equations {
            let mut point = Curve::identity();
            for term in &equation.terms {
                let nonce = nonces.get(usize::from(term.response))?;
                let base = Curve::point(&term.base)?;
                point += base * nonce;
            }
            commitment.push(Curve::encode(&point));
        }
        let payload = bcs::to_bytes(&(statement, context, &commitment)).ok()?;
        let challenge = Hs::scalar(domain, &payload);
        let mut response = Vec::with_capacity(secrets.len());
        for (secret, nonce) in secrets.iter().zip(nonces) {
            response.push((*nonce + challenge * *secret).to_bytes());
        }
        Some(Self {
            commitment,
            response,
        })
    }
}
