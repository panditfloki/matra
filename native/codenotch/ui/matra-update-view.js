// An unchecked or failed request must never say "up to date".
(() => {
  window.matraUpdateView = (s = {}) => {
    const phase = s.phase || 'idle';
    const busy = ['checking', 'downloading', 'installing'].includes(phase);
    const percent = s.total > 0 ? Math.min(100, Math.max(0, Math.floor(s.downloaded * 100 / s.total))) : null;
    const command = phase === 'ready' ? 'install_update' : phase === 'available' ? 'download_update' : 'check_for_update';
    const button = phase === 'ready' ? 'Install & restart' : phase === 'available' ? 'Download update' : phase === 'downloading' ? 'Downloading…' : phase === 'installing' ? 'Installing…' : 'Check for updates';
    const message = phase === 'downloading' ? (percent == null ? 'Starting download…' : `Downloading ${percent}%`) :
      s.message || ({idle:'Not checked yet.', checking:'Checking GitHub…', current:'You have the latest stable release.',
        available:`v${s.available || ''} is available.`, ready:'SHA-256 verified. Ready to install.',
        installing:'Windows Setup will show progress and restart Mātrā.', error:'Update check failed. Retry.'}[phase] || 'Not checked yet.');
    return { phase, busy, percent, command, button, message };
  };
})();
