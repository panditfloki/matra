import Foundation
import XCTest
@testable import Matra

final class MatraLiveStartupTests: XCTestCase {
    func testOrdinaryLaunchIsLiveAndPreviewIsExplicit() {
        XCTAssertFalse(Runtime.usesDesignPreview(environment: [:]))
        XCTAssertFalse(Runtime.usesDesignPreview(environment: ["MATRA_DESIGN_PREVIEW": "0"]))
        XCTAssertTrue(Runtime.usesDesignPreview(environment: ["MATRA_DESIGN_PREVIEW": "1"]))
        XCTAssertTrue(Runtime.usesDesignPreview(environment: [
            "MATRA_DESIGN_PREVIEW": "1", "MATRA_LIVE_PROVIDERS": "1"
        ]))
        XCTAssertTrue(Runtime.isUnderTest)
    }

    func testAppOwnedStorageUsesDistinctProductNamespaces() {
        let support = URL(fileURLWithPath: "/test/Application Support", isDirectory: true)
        XCTAssertEqual(MatraStorage.supportRoot(in: support).path, "/test/Application Support/Matra")
        XCTAssertEqual(ClaudeUsageCLI.scratchLocation(applicationSupport: support).path,
                       "/test/Application Support/Matra/usage-scratch")
        let services = [CustomEndpoint.keychainService, PhoneLinkKeychainSecretStore.service,
                        LMStudioCredentials.keychainService, ApifyCredentials.keychainService,
                        OllamaCredentials.keychainService,
                        MiniMaxCredentials.apiKeyService, MiniMaxCredentials.cookieService]
        XCTAssertEqual(Set(services).count, services.count)
        for service in services {
            XCTAssertTrue(service.hasPrefix("com.dydxfx.matra.mac."))
        }
        XCTAssertEqual(LMStudioCredentials.keychainAccount, "matra")
        XCTAssertEqual(ApifyCredentials.keychainAccount, "matra")
        XCTAssertEqual(OllamaCredentials.keychainAccount, "matra")
        XCTAssertEqual(MiniMaxCredentials.keychainAccount, "matra")
    }
}
