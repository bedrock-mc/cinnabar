"""Round-trip the platform-trust Java helper against the host JDK's accepted certificates."""

import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


class AndroidTrustTests(unittest.TestCase):
    def test_default_trust_round_trips_as_pem(self):
        javac, java = shutil.which("javac"), shutil.which("java")
        if not javac or not java:
            self.skipTest("missing fixture: javac/java for Android platform trust export")
        application_id = json.loads((ROOT / "packaging/android/runtime.json").read_text())["application_id"]
        source = (ROOT / "packaging/android/PlatformTrust.java").read_text()
        fixture = """package PACKAGE;
import java.io.ByteArrayInputStream;
import java.nio.charset.StandardCharsets;
import java.security.KeyStore;
import java.security.cert.Certificate;
import java.security.cert.CertificateFactory;
import java.util.Base64;
import java.util.HashSet;
import java.util.Set;
import javax.net.ssl.TrustManager;
import javax.net.ssl.TrustManagerFactory;
import javax.net.ssl.X509TrustManager;

/** Checks the exported PEM against the platform's accepted certificate set. */
public final class PlatformTrustTest {
    public static void main(String[] args) throws Exception {
        Set<String> exported = new HashSet<String>();
        for (Certificate certificate : CertificateFactory.getInstance("X.509")
                .generateCertificates(new ByteArrayInputStream(
                        PlatformTrust.pem().getBytes(StandardCharsets.US_ASCII)))) {
            exported.add(Base64.getEncoder().encodeToString(certificate.getEncoded()));
        }
        TrustManagerFactory factory = TrustManagerFactory.getInstance(
                TrustManagerFactory.getDefaultAlgorithm());
        factory.init((KeyStore) null);
        Set<String> accepted = new HashSet<String>();
        for (TrustManager manager : factory.getTrustManagers()) {
            if (!(manager instanceof X509TrustManager)) continue;
            for (Certificate certificate : ((X509TrustManager) manager).getAcceptedIssuers()) {
                accepted.add(Base64.getEncoder().encodeToString(certificate.getEncoded()));
            }
            break;
        }
        if (exported.isEmpty() || !exported.equals(accepted)) {
            throw new AssertionError("export differs from the platform trust store");
        }
        System.out.println("platform trust PEM round trip ok");
    }
}
""".replace("PACKAGE", application_id)
        with tempfile.TemporaryDirectory(prefix="cinnabar-platform-trust-") as temporary:
            directory = Path(temporary)
            helper = directory / "PlatformTrust.java"
            helper.write_text(source.replace("@APPLICATION_ID@", application_id))
            test = directory / "PlatformTrustTest.java"
            test.write_text(fixture)
            compiled = subprocess.run(
                [javac, "--release", "8", "-d", temporary, str(helper), str(test)],
                text=True, capture_output=True, timeout=60,
            )
            self.assertEqual(compiled.returncode, 0, compiled.stderr)
            result = subprocess.run(
                [java, "-cp", temporary, f"{application_id}.PlatformTrustTest"],
                text=True, capture_output=True, timeout=60,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("platform trust PEM round trip ok", result.stdout)
