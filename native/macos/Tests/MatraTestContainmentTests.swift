import XCTest
@testable import Matra

final class MatraTestContainmentTests: XCTestCase {
    func testConfiguredFixturePathsStayInsideWorkspace() throws {
        guard let path = ProcessInfo.processInfo.environment["MATRA_TEST_WORKSPACE"] else {
            throw XCTSkip("Workspace isolation assertions apply to the contained validation runner")
        }
        let root = URL(fileURLWithPath: path).resolvingSymlinksInPath().path + "/"
        XCTAssertTrue(MatraTestFiles.temporaryDirectory.resolvingSymlinksInPath().path.hasPrefix(root))
        let support = try XCTUnwrap(FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first)
        XCTAssertTrue(support.resolvingSymlinksInPath().path.hasPrefix(root))
        XCTAssertTrue(Runtime.isUnderTest)
    }
}

/// Darwin can replace TMPDIR when Xcode starts its test host. Fixture writers
/// use an explicit project folder so the validation runner owns every file.
enum MatraTestFiles {
    static let temporaryDirectory: URL = {
        let root = ProcessInfo.processInfo.environment["MATRA_TEST_WORKSPACE"]
            .map { URL(fileURLWithPath: $0) }
            ?? URL(fileURLWithPath: #filePath).deletingLastPathComponent()
                .deletingLastPathComponent().appendingPathComponent(".test-runtime")
        let directory = root.appendingPathComponent("tmp", isDirectory: true)
        do { try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true) }
        catch { preconditionFailure("Cannot create the project's test fixture directory: \(error)") }
        return directory
    }()
}
