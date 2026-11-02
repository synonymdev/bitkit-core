use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use prost::Message;
use trezor_connect_rs::protos::{bitcoin as proto, common, MessageType};
use trezor_connect_rs::transport::traits::{DeviceDescriptor, Transport};
use trezor_connect_rs::{ConnectedDevice, DeviceInfo, Result};

use super::TrezorManager;

pub(super) const PUBLIC_KEY: &str =
    "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
pub(super) const XPUB: &str = "xpub661MyMwAqRbcFtXgS5sYJABqqG9YLmC4Q1Rdap9gSE8NqtwybGhePY2gZ29ESFjqJoCu1Rupje8YtGqsefD265TMg7usUDFdp6W1EGMcet8";

pub(super) struct Exchange {
    pub request: MessageType,
    pub response: MessageType,
    pub payload: Vec<u8>,
}

impl Exchange {
    pub fn new(request: MessageType, response: MessageType, payload: impl Message) -> Self {
        Self {
            request,
            response,
            payload: payload.encode_to_vec(),
        }
    }
}

type Calls = Vec<(u16, Vec<u8>)>;

#[derive(Clone, Default)]
pub(super) struct TestTransport {
    exchanges: Arc<Mutex<VecDeque<Exchange>>>,
    calls: Arc<Mutex<Calls>>,
}

impl TestTransport {
    pub fn manager(exchanges: Vec<Exchange>) -> (TrezorManager, Self) {
        let transport = Self {
            exchanges: Arc::new(Mutex::new(exchanges.into())),
            ..Self::default()
        };
        let device = ConnectedDevice::new(
            DeviceInfo::new_usb("test".into(), 0x1209, 0x53c1),
            Box::new(transport.clone()),
            "test-session".into(),
        );
        (TrezorManager::with_test_device(device), transport)
    }

    pub fn calls(&self) -> Calls {
        self.calls.lock().unwrap().clone()
    }

    pub fn assert_finished(&self) {
        assert!(self.exchanges.lock().unwrap().is_empty());
    }
}

#[async_trait]
impl Transport for TestTransport {
    async fn init(&mut self) -> Result<()> {
        Ok(())
    }

    async fn enumerate(&self) -> Result<Vec<DeviceDescriptor>> {
        Ok(vec![])
    }

    async fn acquire(&self, _path: &str, _previous: Option<&str>) -> Result<String> {
        Ok("test-session".into())
    }

    async fn release(&self, _session: &str) -> Result<()> {
        Ok(())
    }

    async fn call(&self, _session: &str, message_type: u16, data: &[u8]) -> Result<(u16, Vec<u8>)> {
        self.calls
            .lock()
            .unwrap()
            .push((message_type, data.to_vec()));
        let exchange = self
            .exchanges
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected device request");
        assert_eq!(message_type, exchange.request as u16);
        Ok((exchange.response as u16, exchange.payload))
    }

    fn stop(&mut self) {}
}

pub(super) fn public_key_reply(xpub: String, descriptor: Option<String>) -> proto::PublicKey {
    proto::PublicKey {
        node: common::HdNodeType {
            depth: 3,
            fingerprint: 42,
            child_num: 0x80000000,
            chain_code: vec![7; 32],
            private_key: None,
            public_key: hex::decode(PUBLIC_KEY).unwrap(),
        },
        xpub,
        root_fingerprint: Some(0x73c5da0a),
        descriptor,
    }
}

pub(super) fn device_operations() -> Vec<Exchange> {
    vec![
        Exchange::new(
            MessageType::GetAddress,
            MessageType::Address,
            proto::Address {
                address: "test-address".into(),
                mac: None,
            },
        ),
        Exchange::new(
            MessageType::GetPublicKey,
            MessageType::PublicKey,
            public_key_reply(XPUB.into(), None),
        ),
        Exchange::new(
            MessageType::SignMessage,
            MessageType::MessageSignature,
            proto::MessageSignature {
                address: "test-address".into(),
                signature: vec![1],
            },
        ),
    ]
}
