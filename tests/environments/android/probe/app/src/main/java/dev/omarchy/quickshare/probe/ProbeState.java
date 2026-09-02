package dev.omarchy.quickshare.probe;

import com.google.android.gms.nearby.connection.Payload;

import org.json.JSONObject;

import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;

final class ProbeState {
    private final Map<String, Payload> payloads = new ConcurrentHashMap<>();
    private final Map<String, String> values = new ConcurrentHashMap<>();

    void clear() {
        payloads.clear();
        values.clear();
    }

    void record(String category, String identifier, String value) {
        values.put(category + ":" + identifier, value);
    }

    void recordPayload(Payload payload) {
        payloads.put(Long.toString(payload.getId()), payload);
        record(
            "payload-type",
            Long.toString(payload.getId()),
            Integer.toString(payload.getType())
        );
    }

    Payload payload(long identifier) {
        return payloads.get(Long.toString(identifier));
    }

    String snapshot() {
        return new JSONObject(values).toString();
    }
}
