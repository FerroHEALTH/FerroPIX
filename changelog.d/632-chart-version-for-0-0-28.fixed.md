- The Helm chart moves to 0.3.21 for FerroPIX 0.0.28. The v0.0.28 release
  raised the chart's `appVersion` without raising the chart version, so its
  chart leg refused to publish over 0.3.20. A pull request that changes the
  packaged chart now has to raise its version.
