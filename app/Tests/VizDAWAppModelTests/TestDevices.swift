import VizDAWCore
@testable import VizDAWAppModel

/// The input pickers' device reads, stubbed once per test process (Task 3
/// review I2): no Swift test reads the real MIDI sources or the HAL's
/// input channels. Once only, so a suite running in parallel never undoes
/// the stubs `LaneInputsTests` sets for its own tests.
enum TestDevices {
    static let stubbed: Void = {
        LaneInputs.midiPorts = { [] }
        LaneInputs.deviceChannels = { nil }
    }()
}
