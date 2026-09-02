.PHONY: oracle-provision oracle-reference-provision oracle-up oracle-down
.PHONY: nearshare-provision nearshare-up nearshare-down
.PHONY: nearby-linux-provision nearby-linux-up nearby-linux-down
.PHONY: diverse-lan-up diverse-lan-down test-diverse-lan
.PHONY: proxy-provision proxy-up proxy-down
.PHONY: dbus-provision dbus-up dbus-down
.PHONY: bluetooth-radio-provision bluetooth-radio-up bluetooth-radio-down
.PHONY: network-provision network-up network-down
.PHONY: android-preflight android-bootstrap android-license android-provision
.PHONY: android-orchestrator-provision
.PHONY: android-seed android-up android-down test-android-nearby
.PHONY: lint-live-bwu-kvm live-bwu-kvm-provision
.PHONY: live-bwu-kvm-up live-bwu-kvm-down
.PHONY: test-live-bwu-kvm

oracle-provision: ## Build the pinned oracle toolchain image.
	@node tests/environments/oracle/environment.mjs provision

oracle-reference-provision: ## Build the pinned Google UKEY2 artifacts.
	@node tests/environments/oracle/environment.mjs reference-provision

oracle-up: ## Start and readiness-check the prepared oracle environment.
	@node tests/environments/oracle/environment.mjs up

oracle-down: ## Stop the prepared oracle environment within 60 seconds.
	@node tests/environments/oracle/environment.mjs down

nearshare-provision: ## Build the pinned implementation-diverse LAN peer.
	@node tests/environments/nearshare/environment.mjs provision

nearshare-up: ## Start and prepare the pinned NearShare peer.
	@node tests/environments/nearshare/environment.mjs up

nearshare-down: ## Stop the prepared NearShare peer.
	@node tests/environments/nearshare/environment.mjs down

nearby-linux-provision: ## Build the pinned Google-derived Linux peers.
	@node tests/environments/nearby-linux/environment.mjs provision

nearby-linux-up: ## Start and readiness-check the prepared Linux peers.
	@node tests/environments/nearby-linux/environment.mjs up

nearby-linux-down: ## Stop the prepared Linux peers and remove case data.
	@node tests/environments/nearby-linux/environment.mjs down

diverse-lan-up: ## Start the isolated NearShare and Google-derived peers.
	@node tests/environments/diverse-lan/environment.mjs up

diverse-lan-down: ## Stop the isolated diverse-LAN peers and remove case data.
	@node tests/environments/diverse-lan/environment.mjs down

proxy-provision: ## Build pinned Toxiproxy and prove its offline rebuild.
	@node tests/environments/proxies/environment.mjs provision

proxy-up: ## Start and readiness-check Toxiproxy, measured outside test time.
	@node tests/environments/proxies/environment.mjs up

proxy-down: ## Stop Toxiproxy, measured outside test time.
	@node tests/environments/proxies/environment.mjs down

dbus-provision: ## Build the pinned BlueZ and NetworkManager private-bus image.
	@node tests/environments/bluez/dbus-environment.mjs provision

dbus-up: ## Start and readiness-check the private D-Bus environment.
	@node tests/environments/bluez/dbus-environment.mjs up

dbus-down: ## Stop the private D-Bus environment outside test time.
	@node tests/environments/bluez/dbus-environment.mjs down

bluetooth-radio-provision: ## Build the pinned BlueZ and Bumble radio image.
	@node tests/environments/bluez/radio-environment.mjs provision

bluetooth-radio-up: ## Boot and readiness-check the isolated radio guest.
	@node tests/environments/bluez/radio-environment.mjs up

bluetooth-radio-down: ## Stop the isolated radio guest and report teardown.
	@node tests/environments/bluez/radio-environment.mjs down

network-provision: ## Build pinned Wi-Fi tools and deterministic wmediumd.
	@node tests/environments/network/environment.mjs provision

network-up: ## Start isolated hwsim radios and report readiness time.
	@node tests/environments/network/environment.mjs up

network-down: ## Remove isolated radios and report teardown time.
	@node tests/environments/network/environment.mjs down

android-preflight: ## Check host support for the pinned Android AVD lab.
	@$(TIMEOUT) node tests/environments/android/environment.mjs preflight

android-bootstrap: ## Fetch and verify the pinned Android host tools.
	@node tests/environments/android/environment.mjs bootstrap

android-orchestrator-provision: ## Build the pinned Mobly controller image.
	@node tests/environments/android/environment.mjs orchestrator-provision

android-license: ## Interactively review the Android SDK license.
	@node tests/environments/android/environment.mjs license

android-provision: ## Install the SDK, create AVDs, and build the probe.
	@node tests/environments/android/environment.mjs provision

android-seed: ## Cold-boot both AVDs and save their Quick Boot state.
	@node tests/environments/android/environment.mjs seed

android-up: ## Boot both AVDs, wait for readiness, and install the probe.
	@node tests/environments/android/environment.mjs up

android-down: ## Stop both AVDs and report teardown time.
	@node tests/environments/android/environment.mjs down

lint-live-bwu-kvm: ## Check the isolated live-BWU KVM harness contracts.
	@$(TIMEOUT) node --test \
		tests/environments/live-bwu-kvm/contracts/*.test.mjs

live-bwu-kvm-provision: ## Build the sealed Google-peer KVM image.
	@node tests/environments/live-bwu-kvm/environment.mjs provision

live-bwu-kvm-up: ## Start the prepared isolated live-BWU KVM peers.
	@node tests/environments/live-bwu-kvm/environment.mjs up

live-bwu-kvm-down: ## Stop the isolated live-BWU KVM peers.
	@node tests/environments/live-bwu-kvm/environment.mjs down

test-live-bwu-kvm: ## Test LAN and Classic bytes across isolated KVM peers.
	@trap 'node tests/environments/live-bwu-kvm/environment.mjs down' EXIT; \
		node tests/environments/live-bwu-kvm/environment.mjs up; \
		$(TIMEOUT) node tests/environments/live-bwu-kvm/environment.mjs \
		self-test

verify-tooling: lint-live-bwu-kvm
verify: test-live-bwu-kvm
