export function SettingsAboutSection() {
  const version = __APP_VERSION__;
  const commitHash = __APP_COMMIT_HASH__;
  const buildDate = __APP_BUILD_DATE__;
  const gitBranch = __APP_GIT_BRANCH__;

  const formattedBuildDate = (() => {
    try {
      return new Date(buildDate).toLocaleString(undefined, {
        dateStyle: "medium",
        timeStyle: "short",
      });
    } catch {
      return buildDate;
    }
  })();

  return (
    <section className="settings-section">
      <div className="settings-section-title">About</div>
      <div className="settings-section-subtitle">
        Build information and version details.
      </div>
      <div className="settings-about-grid">
        <div className="settings-about-row">
          <span className="settings-about-label">Version</span>
          <span className="settings-about-value">{version}</span>
        </div>
        <div className="settings-about-row">
          <span className="settings-about-label">Commit</span>
          <span className="settings-about-value settings-about-mono">{commitHash}</span>
        </div>
        <div className="settings-about-row">
          <span className="settings-about-label">Branch</span>
          <span className="settings-about-value settings-about-mono">{gitBranch}</span>
        </div>
        <div className="settings-about-row">
          <span className="settings-about-label">Build Date</span>
          <span className="settings-about-value">{formattedBuildDate}</span>
        </div>
      </div>
    </section>
  );
}
