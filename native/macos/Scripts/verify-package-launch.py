#!/usr/bin/env python3
"""Launch the actual Mac package outside a debugger, with provider startup off.

Uses the app's existing XCTest startup guard; it does not run the unit suite.
An OS sandbox denies network and writes outside the explicit evidence directory.
This checks loader compatibility, not Gatekeeper, the updater or live accounts.
"""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("app", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--arch", choices=("arm64", "x86_64"))
    args = parser.parse_args()
    app, output = args.app.resolve(strict=True), args.output.resolve()
    executable = app / "Contents/MacOS/Matra"
    if not executable.is_file() or app.suffix != ".app":
        parser.error("Expected a complete Matra.app bundle")
    if output == app or app in output.parents:
        parser.error("Evidence must be outside the signed bundle")
    output.mkdir(parents=True, exist_ok=True)
    (output / "tmp").mkdir(exist_ok=True)
    subprocess.run(["/usr/bin/codesign", "--verify", "--deep", "--strict", str(app)], check=True)
    signature = subprocess.run(["/usr/bin/codesign", "-dvv", str(app)],
                               capture_output=True, text=True, check=True).stderr
    (output / "signature.txt").write_text(signature)
    match = re.search(r"flags=0x([0-9a-fA-F]+)", signature)
    flags = int(match.group(1), 16) if match else 0
    if "Signature=adhoc" in signature and flags & 0x10000:
        raise RuntimeError("Ad-hoc test package retained hardened runtime; Sparkle can fail before main")
    profile = output / "launch.sb"
    profile.write_text('(version 1)\n(allow default)\n(deny network*)\n'
                       '(deny file-write*)\n(allow file-write* (subpath '
                       + json.dumps(str(output)) + '))\n')
    env = dict(os.environ)
    env["XCTestConfigurationFilePath"] = str(output / "provider-startup-disabled.xctestconfiguration")
    env["TMPDIR"] = str(output / "tmp") + "/"
    command = ["/usr/bin/sandbox-exec", "-f", str(profile)]
    if args.arch:
        command += ["/usr/bin/arch", "-" + args.arch]
    command.append(str(executable))
    evidence = {"app": str(app), "architecture": args.arch or "native",
                "provider_startup_disabled": True, "network_denied": True,
                "signature_verified": True, "duration_seconds": 8}
    with (output / "launch.log").open("w") as log:
        process = subprocess.Popen(command, env=env, stdout=log, stderr=subprocess.STDOUT)
        try:
            deadline = time.monotonic() + 8
            while process.poll() is None and time.monotonic() < deadline:
                time.sleep(0.1)
            evidence["alive_after_8_seconds"] = process.poll() is None
            if process.poll() is not None:
                evidence["premature_exit"] = process.returncode
            else:
                mappings = subprocess.run(["/usr/sbin/lsof", "-a", "-p", str(process.pid),
                                           "-d", "txt", "-Fn"], capture_output=True, text=True)
                (output / "mapped-files.txt").write_text(mappings.stdout)
                evidence["sparkle_mapped"] = "Sparkle.framework/Versions/B/Sparkle" in mappings.stdout
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
            evidence["cleanup_exit"] = process.returncode
    evidence["passed"] = evidence["alive_after_8_seconds"] and evidence.get("sparkle_mapped", False)
    (output / "result.json").write_text(json.dumps(evidence, indent=2) + "\n")
    print(json.dumps(evidence, indent=2))
    return 0 if evidence["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
