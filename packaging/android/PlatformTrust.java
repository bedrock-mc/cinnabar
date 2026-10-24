package @APPLICATION_ID@;

import java.nio.charset.StandardCharsets;
import java.security.GeneralSecurityException;
import java.security.KeyStore;
import java.security.cert.X509Certificate;
import java.util.Base64;
import javax.net.ssl.TrustManager;
import javax.net.ssl.TrustManagerFactory;
import javax.net.ssl.X509TrustManager;

/** Exports the app's platform trust for TLS stacks that cannot read Android's certificate store. */
final class PlatformTrust {
    private PlatformTrust() {}

    /**
     * Returns every issuer the app's default X.509 trust manager accepts, as concatenated PEM
     * certificate blocks.
     *
     * <p>The trust manager comes from the default {@link TrustManagerFactory} initialized with no
     * key store, so the set is whatever the platform trusts for this app's ordinary HTTPS. It
     * contains certificates only, never private keys.
     *
     * @throws GeneralSecurityException when the platform has no X.509 trust manager, when it
     *     accepts no issuers, or when a certificate cannot be encoded.
     */
    static String pem() throws GeneralSecurityException {
        TrustManagerFactory factory =
                TrustManagerFactory.getInstance(TrustManagerFactory.getDefaultAlgorithm());
        factory.init((KeyStore) null);
        X509TrustManager trust = null;
        // TLS uses the first X.509 trust manager, so later ones would not change what is trusted.
        for (TrustManager manager : factory.getTrustManagers()) {
            if (manager instanceof X509TrustManager) {
                trust = (X509TrustManager) manager;
                break;
            }
        }
        if (trust == null) throw new GeneralSecurityException("no X.509 trust manager");
        X509Certificate[] issuers = trust.getAcceptedIssuers();
        if (issuers == null || issuers.length == 0) {
            throw new GeneralSecurityException("the platform trust manager accepts no issuers");
        }
        Base64.Encoder encoder = Base64.getMimeEncoder(64, "\n".getBytes(StandardCharsets.US_ASCII));
        StringBuilder pem = new StringBuilder(issuers.length * 2048);
        for (X509Certificate issuer : issuers) {
            pem.append("-----BEGIN CERTIFICATE-----\n")
                    .append(encoder.encodeToString(issuer.getEncoded()))
                    .append("\n-----END CERTIFICATE-----\n");
        }
        return pem.toString();
    }
}
