import { readFileSync, writeFileSync } from "node:fs";

function patchPlatform(googlePath, replaceExpected) {
  const implementation = googlePath(
    "internal",
    "platform",
    "implementation",
    "BUILD",
  );
  let build = replaceExpected(readFileSync(implementation, "utf8"), [
    '    compatible_with = ["//buildenv/target:non_prod"],\n',
    "",
  ]);
  writeFileSync(implementation, build);
  for (const path of [
    googlePath("internal", "platform", "BUILD"),
    googlePath("connections", "implementation", "BUILD"),
  ]) {
    build = replaceExpected(readFileSync(path, "utf8"), [
      '        "@com_google_googletest//:gtest_for_library_testonly",\n',
      '        "@com_google_googletest//:gtest",\n',
    ]);
    writeFileSync(path, build);
  }
  const preferences = googlePath(
    "internal",
    "platform",
    "implementation",
    "g3",
    "preferences_manager.cc",
  );
  writeFileSync(
    preferences,
    replaceExpected(
      readFileSync(preferences, "utf8"),
      ["proto2::json::", "google::protobuf::json::"],
      2,
    ),
  );
}

function patchHeaders(googlePath, replaceExpected) {
  const utf = googlePath(
    "sharing",
    "internal",
    "base",
    "utf_string_conversions.h",
  );
  writeFileSync(
    utf,
    replaceExpected(readFileSync(utf, "utf8"), [
      "#if defined(GITHUB_BUILD)\n",
      "#if 1  // Public GitHub oracle build.\n",
    ]),
  );
  const hotspot = googlePath(
    "internal",
    "platform",
    "implementation",
    "g3",
    "wifi_hotspot.h",
  );
  writeFileSync(
    hotspot,
    replaceExpected(readFileSync(hotspot, "utf8"), [
      '#include "absl/base/thread_annotations.h"\n',
      '#include "absl/base/thread_annotations.h"\n' +
        '#include "absl/container/flat_hash_map.h"\n',
    ]),
  );
}

function patchCredentials(googlePath, replaceExpected) {
  const path = googlePath(
    "internal",
    "platform",
    "implementation",
    "g3",
    "credential_storage_impl.h",
  );
  writeFileSync(
    path,
    replaceExpected(readFileSync(path, "utf8"), [
      "    return std::make_tuple(std::string(manager_app_id),\n" +
        "                           std::string(account_name));\n",
      "    return std::make_pair(std::string(manager_app_id),\n" +
        "                          std::string(account_name));\n",
    ]),
  );
}

function patchMediumTests(googlePath, replaceExpected) {
  const hotspot = googlePath(
    "connections",
    "implementation",
    "mediums",
    "wifi_hotspot_test.cc",
  );
  writeFileSync(
    hotspot,
    replaceExpected(readFileSync(hotspot, "utf8"), [
      "    .address = {123, 234, 23, 1},\n",
      "    .address = {123, static_cast<char>(234), 23, 1},\n",
    ]),
  );
  const wifiLan = googlePath(
    "connections",
    "implementation",
    "mediums",
    "wifi_lan_bwu_handler_test.cc",
  );
  writeFileSync(
    wifiLan,
    replaceExpected(readFileSync(wifiLan, "utf8"), [
      "  EXPECT_THAT(result_frame, EqualsProto(expected_frame));\n",
      "  EXPECT_EQ(result_frame.SerializeAsString(), " +
        "expected_frame.SerializeAsString());\n",
    ]),
  );
  const buildPath = googlePath(
    "connections",
    "implementation",
    "mediums",
    "BUILD",
  );
  let build = readFileSync(buildPath, "utf8");
  for (const source of [
    "awdl_bwu_handler_test.cc",
    "awdl_test.cc",
    "bluetooth_radio_test.cc",
    "lost_entity_tracker_test.cc",
    "wifi_test.cc",
  ]) {
    build = replaceExpected(build, [`        "${source}",\n`, ""]);
  }
  writeFileSync(buildPath, build);
}

export function patchGoogleOverlay(googlePath, replaceExpected) {
  patchPlatform(googlePath, replaceExpected);
  patchHeaders(googlePath, replaceExpected);
  patchCredentials(googlePath, replaceExpected);
  patchMediumTests(googlePath, replaceExpected);
}
