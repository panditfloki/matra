import XCTest
@testable import Matra

final class CustomEndpointTransportTests: XCTestCase {
    func testLocalRuntimeNetworkingDoesNotDisableTransportSecurity() throws {
        let ats = try XCTUnwrap(Bundle.main.infoDictionary?["NSAppTransportSecurity"] as? [String: Any])
        XCTAssertNotEqual(ats["NSAllowsArbitraryLoads"] as? Bool, true)
        XCTAssertEqual(ats["NSAllowsLocalNetworking"] as? Bool, true)
        XCTAssertNil(ats["NSExceptionDomains"])
    }
}
