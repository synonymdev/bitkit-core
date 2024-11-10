use bitcoin::address::{Address, NetworkUnchecked};
use bitcoin::Network;
use std::str::FromStr;
use crate::modules::scanner::{NetworkType};
use crate::onchain::AddressError;
use super::types::{AddressType, ValidationResult};

pub struct BitcoinAddressValidator;

impl BitcoinAddressValidator {
    pub fn validate_address(address: &str) -> Result<ValidationResult, AddressError> {
        println!("\nValidating address: {}", address);

        let unchecked_addr = match parse_address(address) {
            Ok(addr) => addr,
            Err(e) => return Err(e),
        };
        let expected_network = match determine_network(address) {
            Ok(n) => n,
            Err(e) => return Err(e),
        };
        match verify_network(unchecked_addr, expected_network) {
            Ok(_) => {},
            Err(e) => return Err(e),
        }
        let address_type = get_address_type(address)?;

        println!("✓ Validation successful!");

        Ok(ValidationResult {
            address: address.to_string(),
            network: NetworkType::from(expected_network),
            address_type,
        })
    }
}

fn parse_address(address: &str) -> Result<Address<NetworkUnchecked>, AddressError> {
    Address::from_str(address)
        .map_err(|e| {
            println!("✗ Failed to parse address: {:?}", e);
            AddressError::InvalidAddress
        })
        .map(|addr| {
            println!("✓ Successfully parsed address");
            addr
        })
}

fn determine_network(address: &str) -> Result<Network, AddressError> {
    match address {
        s if s.starts_with("1") || s.starts_with("3") || s.starts_with("bc1") => {
            println!("✓ Determined network: Bitcoin");
            Ok(Network::Bitcoin)
        },
        s if s.starts_with("2") || s.starts_with("tb1") || s.starts_with("m") || s.starts_with("n") => {
            println!("✓ Determined network: Testnet");
            Ok(Network::Testnet)
        },
        s if s.starts_with("bcrt1") => {
            println!("✓ Determined network: Regtest");
            Ok(Network::Regtest)
        },
        _ => {
            println!("✗ Could not determine network");
            Err(AddressError::InvalidNetwork)
        }
    }
}

fn verify_network(unchecked_addr: Address<NetworkUnchecked>, expected_network: Network)
                  -> Result<Address, AddressError> {
    println!("Attempting to verify address for network: {:?}", expected_network);
    unchecked_addr.require_network(expected_network)
        .map_err(|e| {
            println!("✗ Network verification failed: {:?}", e);
            AddressError::InvalidNetwork
        })
        .map(|addr| {
            println!("✓ Address verified for network");
            addr
        })
}

fn get_address_type(address: &str) -> Result<AddressType, AddressError> {
    let address_type = match address {
        // Legacy addresses (P2PKH)
        s if s.starts_with("1") || s.starts_with("m") || s.starts_with("n") => Some(AddressType::P2PKH),
        // SegWit addresses (P2SH)
        s if s.starts_with("3") || s.starts_with("2") => Some(AddressType::P2SH),
        // Taproot addresses (P2TR)
        s if s.starts_with("bc1p") || s.starts_with("tb1p") => Some(AddressType::P2TR),
        // Native SegWit addresses (P2WPKH)
        s if (s.starts_with("bc1q") || s.starts_with("tb1q")) && s.len() == 42 => Some(AddressType::P2WPKH),
        // Native SegWit Script addresses (P2WSH)
        s if (s.starts_with("bc1q") || s.starts_with("tb1q")) && s.len() == 62 => Some(AddressType::P2WSH),
        // Regtest addresses
        s if s.starts_with("bcrt1") => {
            if s.len() == 42 {
                Some(AddressType::P2WPKH)
            } else if s.len() == 62 {
                Some(AddressType::P2WSH)
            } else {
                Some(AddressType::Unknown)
            }
        },
        _ => Some(AddressType::Unknown)
    };

    address_type.map(|t| {
        println!("✓ Determined address type: {:?}", t);
        t
    }).ok_or_else(|| {
        println!("✗ Could not determine address type");
        AddressError::InvalidAddress
    })
}
