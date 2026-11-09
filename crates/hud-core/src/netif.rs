//! Which network interfaces count towards the HUD's network throughput.
//!
//! Windows lists, besides each adapter, the NDIS filter layers stacked on it (QoS Packet
//! Scheduler, WFP, ...) as separate interfaces that repeat the adapter's byte counters. Only a
//! real adapter (hardware, not a filter layer) may be summed, otherwise traffic is counted
//! several times.

/// The facts of one interface table row that decide whether it is counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterfaceFacts {
    /// `HardwareInterface` flag: a physical adapter.
    pub hardware: bool,
    /// `FilterInterface` flag: an NDIS filter layer over another interface.
    pub filter: bool,
    /// The media is connected (not reported as disconnected).
    pub connected: bool,
    /// Software loopback interface.
    pub loopback: bool,
}

impl InterfaceFacts {
    /// Decodes `InterfaceAndOperStatusFlags` (bit 0 hardware, bit 1 filter).
    pub fn from_flags(flags: u8, connected: bool, loopback: bool) -> Self {
        Self {
            hardware: flags & 0b01 != 0,
            filter: flags & 0b10 != 0,
            connected,
            loopback,
        }
    }

    /// True when the interface's bytes belong in the network total.
    pub fn is_counted(&self) -> bool {
        self.hardware && !self.filter && self.connected && !self.loopback
    }
}

/// Indexes of the rows that count towards the network total.
pub fn counted_indexes(rows: &[InterfaceFacts]) -> Vec<usize> {
    rows.iter()
        .enumerate()
        .filter(|(_, r)| r.is_counted())
        .map(|(i, _)| i)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HW: u8 = 0b01;
    const FILTER: u8 = 0b10;

    fn row(flags: u8) -> InterfaceFacts {
        InterfaceFacts::from_flags(flags, true, false)
    }

    #[test]
    fn real_machine_table_selects_only_the_wifi_adapter() {
        // Wi-Fi + 5 filter layers, vEthernet (WSL) + 3 filters, 2 WAN miniports, loopback.
        let mut rows = vec![row(HW)];
        rows.extend((0..5).map(|_| row(FILTER)));
        rows.push(row(0));
        rows.extend((0..3).map(|_| row(FILTER)));
        rows.extend((0..2).map(|_| row(0)));
        rows.push(InterfaceFacts::from_flags(0, true, true));
        assert_eq!(counted_indexes(&rows), vec![0]);
    }

    #[test]
    fn ethernet_and_wifi_are_both_counted() {
        let rows = [row(HW), row(FILTER), row(HW), row(FILTER)];
        assert_eq!(counted_indexes(&rows), vec![0, 2]);
    }

    #[test]
    fn disconnected_hardware_is_excluded() {
        let rows = [InterfaceFacts::from_flags(HW, false, false), row(HW)];
        assert_eq!(counted_indexes(&rows), vec![1]);
    }

    #[test]
    fn filter_rows_are_never_counted() {
        assert!(!row(FILTER).is_counted());
        assert!(!row(HW | FILTER).is_counted());
    }

    #[test]
    fn loopback_hardware_is_excluded() {
        assert!(!InterfaceFacts::from_flags(HW, true, true).is_counted());
    }

    #[test]
    fn empty_table_selects_nothing() {
        assert!(counted_indexes(&[]).is_empty());
    }
}
