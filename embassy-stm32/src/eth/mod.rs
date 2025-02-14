#![macro_use]
#![cfg_attr(not(feature = "embassy-net"), allow(unused))]

#[cfg_attr(any(eth_v1a, eth_v1b, eth_v1c), path = "v1/mod.rs")]
#[cfg_attr(eth_v2, path = "v2/mod.rs")]
mod _version;
pub mod generic_smi;

pub use _version::*;
use embassy_sync::waitqueue::AtomicWaker;

#[allow(unused)]
const MTU: usize = 1514;
const TX_BUFFER_SIZE: usize = 1514;
const RX_BUFFER_SIZE: usize = 1536;

#[repr(C, align(8))]
#[derive(Copy, Clone)]
pub(crate) struct Packet<const N: usize>([u8; N]);

pub struct PacketQueue<const TX: usize, const RX: usize> {
    tx_desc: [TDes; TX],
    rx_desc: [RDes; RX],
    tx_buf: [Packet<TX_BUFFER_SIZE>; TX],
    rx_buf: [Packet<RX_BUFFER_SIZE>; RX],
}

impl<const TX: usize, const RX: usize> PacketQueue<TX, RX> {
    pub const fn new() -> Self {
        const NEW_TDES: TDes = TDes::new();
        const NEW_RDES: RDes = RDes::new();
        Self {
            tx_desc: [NEW_TDES; TX],
            rx_desc: [NEW_RDES; RX],
            tx_buf: [Packet([0; TX_BUFFER_SIZE]); TX],
            rx_buf: [Packet([0; RX_BUFFER_SIZE]); RX],
        }
    }
}

static WAKER: AtomicWaker = AtomicWaker::new();

#[cfg(feature = "embassy-net")]
mod embassy_net_impl {
    use core::task::Context;

    use embassy_net::device::{Device, DeviceCapabilities, LinkState};

    use super::*;

    impl<'d, T: Instance, P: PHY> Device for Ethernet<'d, T, P> {
        type RxToken<'a> = RxToken<'a, 'd> where Self: 'a;
        type TxToken<'a> = TxToken<'a, 'd> where Self: 'a;

        fn receive(&mut self, cx: &mut Context) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
            WAKER.register(cx.waker());
            if self.rx.available().is_some() && self.tx.available().is_some() {
                Some((RxToken { rx: &mut self.rx }, TxToken { tx: &mut self.tx }))
            } else {
                None
            }
        }

        fn transmit(&mut self, cx: &mut Context) -> Option<Self::TxToken<'_>> {
            WAKER.register(cx.waker());
            if self.tx.available().is_some() {
                Some(TxToken { tx: &mut self.tx })
            } else {
                None
            }
        }

        fn capabilities(&self) -> DeviceCapabilities {
            let mut caps = DeviceCapabilities::default();
            caps.max_transmission_unit = MTU;
            caps.max_burst_size = Some(self.tx.len());
            caps
        }

        fn link_state(&mut self, cx: &mut Context) -> LinkState {
            // TODO: wake cx.waker on link state change
            cx.waker().wake_by_ref();
            if P::poll_link(self) {
                LinkState::Up
            } else {
                LinkState::Down
            }
        }

        fn ethernet_address(&self) -> [u8; 6] {
            self.mac_addr
        }
    }

    pub struct RxToken<'a, 'd> {
        rx: &'a mut RDesRing<'d>,
    }

    impl<'a, 'd> embassy_net::device::RxToken for RxToken<'a, 'd> {
        fn consume<R, F>(self, f: F) -> R
        where
            F: FnOnce(&mut [u8]) -> R,
        {
            // NOTE(unwrap): we checked the queue wasn't full when creating the token.
            let pkt = unwrap!(self.rx.available());
            let r = f(pkt);
            self.rx.pop_packet();
            r
        }
    }

    pub struct TxToken<'a, 'd> {
        tx: &'a mut TDesRing<'d>,
    }

    impl<'a, 'd> embassy_net::device::TxToken for TxToken<'a, 'd> {
        fn consume<R, F>(self, len: usize, f: F) -> R
        where
            F: FnOnce(&mut [u8]) -> R,
        {
            // NOTE(unwrap): we checked the queue wasn't full when creating the token.
            let pkt = unwrap!(self.tx.available());
            let r = f(&mut pkt[..len]);
            self.tx.transmit(len);
            r
        }
    }
}
/// Station Management Interface (SMI) on an ethernet PHY
///
/// # Safety
///
/// The methods cannot move out of self
pub unsafe trait StationManagement {
    /// Read a register over SMI.
    fn smi_read(&mut self, reg: u8) -> u16;
    /// Write a register over SMI.
    fn smi_write(&mut self, reg: u8, val: u16);
}

/// Traits for an Ethernet PHY
///
/// # Safety
///
/// The methods cannot move S
pub unsafe trait PHY {
    /// Reset PHY and wait for it to come out of reset.
    fn phy_reset<S: StationManagement>(sm: &mut S);
    /// PHY initialisation.
    fn phy_init<S: StationManagement>(sm: &mut S);
    /// Poll link to see if it is up and FD with 100Mbps
    fn poll_link<S: StationManagement>(sm: &mut S) -> bool;
}

pub(crate) mod sealed {
    pub trait Instance {
        fn regs() -> crate::pac::eth::Eth;
    }
}

pub trait Instance: sealed::Instance + Send + 'static {}

impl sealed::Instance for crate::peripherals::ETH {
    fn regs() -> crate::pac::eth::Eth {
        crate::pac::ETH
    }
}
impl Instance for crate::peripherals::ETH {}

pin_trait!(RefClkPin, Instance);
pin_trait!(MDIOPin, Instance);
pin_trait!(MDCPin, Instance);
pin_trait!(CRSPin, Instance);
pin_trait!(RXD0Pin, Instance);
pin_trait!(RXD1Pin, Instance);
pin_trait!(TXD0Pin, Instance);
pin_trait!(TXD1Pin, Instance);
pin_trait!(TXEnPin, Instance);
