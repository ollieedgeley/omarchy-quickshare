package dev.omarchy.quickshare.probe;

import android.content.Context;

import com.google.android.gms.common.api.CommonStatusCodes;
import com.google.android.gms.nearby.connection.ConnectionInfo;
import com.google.android.gms.nearby.connection.ConnectionLifecycleCallback;
import com.google.android.gms.nearby.connection.ConnectionResolution;
import com.google.android.gms.nearby.connection.DiscoveredEndpointInfo;
import com.google.android.gms.nearby.connection.EndpointDiscoveryCallback;
import com.google.android.gms.nearby.connection.Payload;
import com.google.android.gms.nearby.connection.PayloadCallback;
import com.google.android.gms.nearby.connection.PayloadTransferUpdate;

final class NearbyCallbacks {
    final Context context;
    final ProbeState state;

    NearbyCallbacks(Context context, ProbeState state) {
        this.context = context;
        this.state = state;
    }

    EndpointDiscoveryCallback discovery() {
        return new EndpointDiscoveryCallback() {
            @Override
            public void onEndpointFound(
                String endpointId,
                DiscoveredEndpointInfo information
            ) {
                state.record("discovered", endpointId, "found");
            }

            @Override
            public void onEndpointLost(String endpointId) {
                state.record("discovered", endpointId, "lost");
            }
        };
    }

    ConnectionLifecycleCallback connection() {
        return new ConnectionLifecycleCallback() {
            @Override
            public void onConnectionInitiated(
                String endpointId,
                ConnectionInfo information
            ) {
                state.record(
                    "authentication",
                    endpointId,
                    information.getAuthenticationDigits()
                );
                state.record("connection", endpointId, "initiated");
            }

            @Override
            public void onConnectionResult(
                String endpointId,
                ConnectionResolution resolution
            ) {
                int code = resolution.getStatus().getStatusCode();
                String result = "failed:" + code;
                if (code == CommonStatusCodes.SUCCESS) {
                    result = "connected";
                }
                state.record("connection", endpointId, result);
            }

            @Override
            public void onDisconnected(String endpointId) {
                state.record("connection", endpointId, "disconnected");
            }
        };
    }

    PayloadCallback payload() {
        return new PayloadCallback() {
            @Override
            public void onPayloadReceived(
                String endpointId,
                Payload payload
            ) {
                state.recordPayload(payload);
                if (payload.getType() == Payload.Type.BYTES) {
                    PayloadEvidence.captureBytes(state, payload);
                }
                state.record(
                    "payload-source",
                    Long.toString(payload.getId()),
                    endpointId
                );
            }

            @Override
            public void onPayloadTransferUpdate(
                String endpointId,
                PayloadTransferUpdate update
            ) {
                state.record(
                    "payload-status",
                    Long.toString(update.getPayloadId()),
                    Integer.toString(update.getStatus())
                );
                int status = update.getStatus();
                if (status == PayloadTransferUpdate.Status.SUCCESS) {
                    Payload received = state.payload(update.getPayloadId());
                    if (received != null
                        && received.getType() == Payload.Type.FILE) {
                        PayloadEvidence.captureFile(context, state, received);
                    }
                }
            }
        };
    }
}
