package dev.omarchy.quickshare.probe;

import android.content.Context;

import com.google.android.gms.nearby.connection.Payload;

import java.io.IOException;
import java.io.InputStream;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;

final class PayloadEvidence {
    private static final int BUFFER_SIZE = 8192;
    private static final String MISSING_PAYLOAD =
        "received payload URI could not be opened";
    private static final char[] HEX_DIGITS =
        "0123456789abcdef".toCharArray();

    private PayloadEvidence() {
    }

    static void captureBytes(ProbeState state, Payload payload) {
        byte[] bytes = payload.asBytes();
        if (bytes != null) {
            state.record(
                "payload-sha256",
                Long.toString(payload.getId()),
                hex(digest().digest(bytes))
            );
        }
    }

    static void captureFile(
        Context context,
        ProbeState state,
        Payload payload
    ) {
        try (InputStream input = context.getContentResolver().openInputStream(
            payload.asFile().asUri()
        )) {
            if (input == null) {
                throw new IOException(MISSING_PAYLOAD);
            }
            state.record(
                "payload-sha256",
                Long.toString(payload.getId()),
                hash(input)
            );
        } catch (IOException exception) {
            state.record(
                "payload-evidence",
                Long.toString(payload.getId()),
                exception.getClass().getSimpleName()
            );
        }
    }

    private static String hash(InputStream input) throws IOException {
        MessageDigest digest = digest();
        byte[] buffer = new byte[BUFFER_SIZE];
        int length = input.read(buffer);
        while (length >= 0) {
            digest.update(buffer, 0, length);
            length = input.read(buffer);
        }
        return hex(digest.digest());
    }

    private static MessageDigest digest() {
        try {
            return MessageDigest.getInstance("SHA-256");
        } catch (NoSuchAlgorithmException exception) {
            throw new IllegalStateException(
                "SHA-256 is unavailable",
                exception
            );
        }
    }

    private static String hex(byte[] bytes) {
        StringBuilder value = new StringBuilder(bytes.length * 2);
        for (byte current : bytes) {
            int unsigned = current & 0xff;
            value.append(HEX_DIGITS[unsigned >>> 4]);
            value.append(HEX_DIGITS[unsigned & 0x0f]);
        }
        return value.toString();
    }
}
