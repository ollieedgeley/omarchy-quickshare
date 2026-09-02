package dev.omarchy.quickshare.probe;

import android.content.Context;
import android.util.Base64;

import androidx.annotation.NonNull;
import androidx.test.platform.app.InstrumentationRegistry;

import com.google.android.gms.nearby.Nearby;
import com.google.android.gms.nearby.connection.AdvertisingOptions;
import com.google.android.gms.nearby.connection.ConnectionOptions;
import com.google.android.gms.nearby.connection.ConnectionsClient;
import com.google.android.gms.nearby.connection.DiscoveryOptions;
import com.google.android.gms.nearby.connection.Payload;
import com.google.android.gms.nearby.connection.Strategy;
import com.google.android.gms.tasks.Task;
import com.google.android.mobly.snippet.Snippet;
import com.google.android.mobly.snippet.rpc.Rpc;

import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.nio.charset.StandardCharsets;

public final class NearbyProbe implements Snippet {
    private static final String SERVICE_ID =
        "dev.omarchy.quickshare.probe.connections";
    private static final Strategy STRATEGY = Strategy.P2P_POINT_TO_POINT;

    private final Context context;
    private final NearbyCallbacks callbacks;
    private final ProbeState state;
    private ConnectionsClient client;

    public NearbyProbe() {
        context = InstrumentationRegistry.getInstrumentation()
            .getTargetContext();
        state = new ProbeState();
        callbacks = new NearbyCallbacks(context, state);
    }

    @Rpc(description = "Returns a deterministic probe readiness value.")
    public @NonNull String ping() {
        return "ready";
    }

    @Rpc(description = "Returns the current observable probe state as JSON.")
    public @NonNull String snapshot() {
        return state.snapshot();
    }

    @Rpc(description = "Starts advertising a Nearby Connections endpoint.")
    public void startAdvertising(@NonNull String endpointName) {
        AdvertisingOptions options = new AdvertisingOptions.Builder()
            .setStrategy(STRATEGY)
            .build();
        byte[] information = endpointName.getBytes(StandardCharsets.UTF_8);
        track(
            "advertising",
            connections().startAdvertising(
                information,
                SERVICE_ID,
                callbacks.connection(),
                options
            )
        );
    }

    @Rpc(description = "Starts discovering Nearby Connections endpoints.")
    public void startDiscovery() {
        DiscoveryOptions options = new DiscoveryOptions.Builder()
            .setStrategy(STRATEGY)
            .build();
        track(
            "discovery",
            connections().startDiscovery(
                SERVICE_ID,
                callbacks.discovery(),
                options
            )
        );
    }

    @Rpc(description = "Requests an authenticated endpoint connection.")
    public void requestConnection(
        @NonNull String endpointId,
        @NonNull String endpointName
    ) {
        ConnectionOptions options = new ConnectionOptions.Builder().build();
        byte[] information = endpointName.getBytes(StandardCharsets.UTF_8);
        track(
            "request:" + endpointId,
            connections().requestConnection(
                information,
                endpointId,
                callbacks.connection(),
                options
            )
        );
    }

    @Rpc(description = "Accepts an initiated endpoint connection.")
    public void acceptConnection(@NonNull String endpointId) {
        track(
            "accept:" + endpointId,
            connections().acceptConnection(endpointId, callbacks.payload())
        );
    }

    @Rpc(description = "Rejects an initiated endpoint connection.")
    public void rejectConnection(@NonNull String endpointId) {
        track(
            "reject:" + endpointId,
            connections().rejectConnection(endpointId)
        );
    }

    @Rpc(description = "Sends a base64-encoded bytes payload.")
    public long sendBytes(
        @NonNull String endpointId,
        @NonNull String base64
    ) {
        Payload payload = Payload.fromBytes(
            Base64.decode(base64, Base64.NO_WRAP)
        );
        send(endpointId, payload);
        return payload.getId();
    }

    @Rpc(description = "Sends a base64-encoded file payload.")
    public long sendFile(
        @NonNull String endpointId,
        @NonNull String base64
    ) throws IOException {
        File file = new File(context.getFilesDir(), "payload.bin");
        try (FileOutputStream output = new FileOutputStream(file)) {
            output.write(Base64.decode(base64, Base64.NO_WRAP));
        }
        Payload payload = Payload.fromFile(file);
        send(endpointId, payload);
        return payload.getId();
    }

    @Rpc(description = "Cancels an active incoming or outgoing payload.")
    public void cancelPayload(long payloadId) {
        track(
            "cancel:" + payloadId,
            connections().cancelPayload(payloadId)
        );
    }

    @Rpc(description = "Stops connections and clears observable state.")
    public void reset() {
        if (client != null) {
            client.stopAdvertising();
            client.stopDiscovery();
            client.stopAllEndpoints();
        }
        state.clear();
    }

    private ConnectionsClient connections() {
        if (client == null) {
            client = Nearby.getConnectionsClient(context);
        }
        return client;
    }

    private void send(String endpointId, Payload payload) {
        state.record(
            "outgoing-payload",
            Long.toString(payload.getId()),
            endpointId
        );
        track(
            "send:" + payload.getId(),
            connections().sendPayload(endpointId, payload)
        );
    }

    private void track(String operation, Task<Void> task) {
        state.record("operation", operation, "pending");
        task.addOnSuccessListener(
            ignored -> state.record("operation", operation, "succeeded")
        );
        task.addOnFailureListener(
            failure -> state.record(
                "operation",
                operation,
                "failed:" + failure.getClass().getSimpleName()
            )
        );
    }

    @Override
    public void shutdown() {
        reset();
    }
}
