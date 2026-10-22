// @ts-check

import { h } from 'preact';
import { useState, useEffect, useRef } from 'preact/hooks';
import htm from 'htm';
import * as api from '../api.js';
import { Modal } from './modal.js';
import { Checkbox } from './form/checkbox.js';
import { t } from '../i18n.js';

const html = htm.bind(h);

/**
 * @param {{ onComplete: () => void, onCancel: () => void }} props
 */
export function TotpWizard({ onComplete, onCancel }) {
  const [step, setStep] = useState(/** @type {'scan'|'verify'|'codes'} */ ('scan'));
  const [setup, setSetup] = useState(/** @type {{secret:string,otpauth_uri:string,qr_data_url:string}|null} */ (null));
  const [codes, setCodes] = useState(/** @type {string[]} */ ([]));
  const [error, setError] = useState(/** @type {string|null} */ (null));
  const [loading, setLoading] = useState(true);
  const [savedConfirmed, setSavedConfirmed] = useState(false);

  useEffect(() => {
    api.beginTotpSetup()
      .then(data => { setSetup(data); setLoading(false); })
      .catch(e => { setError(e?.message ?? t('totp.setup.start_failed')); setLoading(false); });
  }, []);

  async function handleVerify(code) {
    setError(null);
    try {
      const res = await api.verifyTotpSetup(code);
      setCodes(res.backup_codes ?? []);
      setStep('codes');
    } catch (e) {
      setError(e?.message ?? t('totp.verify.wrong_code'));
    }
  }

  const title = step === 'scan' ? t('totp.step.scan.title')
    : step === 'verify' ? t('totp.step.verify.title')
    : t('totp.step.codes.title');

  return html`
    <${Modal} open=${true} title=${title} onClose=${onCancel}>
      ${loading && html`<div class="py-8 text-center text-text-muted text-sm">${t('totp.setup.loading')}</div>`}
      ${!loading && error && step === 'scan' && html`
        <div class="py-4 text-sm text-danger">${error}</div>
      `}
      ${!loading && setup && step === 'scan' && html`<${ScanStep} setup=${setup} error=${error} onNext=${() => { setError(null); setStep('verify'); }} onCancel=${onCancel} />`}
      ${!loading && step === 'verify' && html`<${VerifyStep} error=${error} onVerify=${handleVerify} onBack=${() => setStep('scan')} />`}
      ${step === 'codes' && html`<${CodesStep} codes=${codes} confirmed=${savedConfirmed} onConfirm=${setSavedConfirmed} onDone=${onComplete} />`}
    </${Modal}>
  `;
}

function ScanStep({ setup, error, onNext, onCancel }) {
  const [copied, setCopied] = useState(false);

  async function copySecret() {
    await navigator.clipboard.writeText(setup.secret).catch(() => {});
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }

  return html`
    <div class="flex flex-col gap-4">
      <p class="text-sm text-text-muted">${t('totp.scan.desc')}</p>
      ${setup.qr_data_url && html`
        <div class="flex justify-center">
          <img src=${setup.qr_data_url} alt=${t('totp.scan.qr_alt')} class="w-48 h-48 rounded border border-border-subtle" />
        </div>
      `}
      <div class="flex items-center gap-2">
        <code class="flex-1 text-xs font-mono bg-surface-raised px-2 py-1.5 rounded truncate select-all">${setup.secret}</code>
        <button type="button" class="btn-ghost btn-sm shrink-0" onClick=${copySecret}>
          ${copied ? t('common.copied') : t('common.copy')}
        </button>
      </div>
      ${error && html`<div class="text-sm text-danger">${error}</div>`}
      <div class="flex gap-2 justify-end mt-2">
        <button type="button" class="btn-ghost btn-sm" onClick=${onCancel}>${t('common.cancel')}</button>
        <button type="button" class="btn-primary btn-sm" onClick=${onNext}>${t('totp.action.next')}</button>
      </div>
    </div>
  `;
}

function VerifyStep({ error, onVerify, onBack }) {
  const [code, setCode] = useState('');
  const inputRef = useRef(null);

  useEffect(() => { inputRef.current?.focus(); }, []);

  function handleSubmit(e) {
    e.preventDefault();
    if (code.length === 6) onVerify(code);
  }

  return html`
    <form onSubmit=${handleSubmit} class="flex flex-col gap-4">
      <p class="text-sm text-text-muted">${t('totp.verify.desc')}</p>
      <input
        ref=${inputRef}
        type="text"
        inputMode="numeric"
        pattern="[0-9]{6}"
        maxLength="6"
        class="input text-center text-2xl tracking-widest font-mono"
        placeholder="000000"
        value=${code}
        onInput=${e => setCode(e.target.value.replace(/\D/g, '').slice(0, 6))}
        autocomplete="one-time-code"
      />
      ${error && html`<div class="text-sm text-danger">${error}</div>`}
      <div class="flex gap-2 justify-end mt-2">
        <button type="button" class="btn-ghost btn-sm" onClick=${onBack}>${t('totp.action.back')}</button>
        <button type="submit" class="btn-primary btn-sm" disabled=${code.length !== 6}>${t('totp.action.verify')}</button>
      </div>
    </form>
  `;
}

function CodesStep({ codes, confirmed, onConfirm, onDone }) {
  const [copied, setCopied] = useState(false);

  async function copyAll() {
    await navigator.clipboard.writeText(codes.join('\n')).catch(() => {});
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }

  function downloadCodes() {
    const blob = new Blob([codes.join('\n')], { type: 'text/plain' });
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = 'kani-backup-codes.txt';
    a.click();
    URL.revokeObjectURL(a.href);
  }

  return html`
    <div class="flex flex-col gap-4">
      <p class="text-sm text-text">
        <strong>${t('totp.codes.desc_bold')}</strong> ${t('totp.codes.desc_rest')}
      </p>
      <div class="grid grid-cols-2 gap-2 font-mono text-sm">
        ${codes.map(c => html`
          <div key=${c} class="bg-surface-raised rounded px-2 py-1.5 text-center select-all">${c}</div>
        `)}
      </div>
      <div class="flex gap-2">
        <button type="button" class="btn-ghost btn-sm flex-1" onClick=${copyAll}>${copied ? t('common.copied') : t('totp.codes.copy_all')}</button>
        <button type="button" class="btn-ghost btn-sm flex-1" onClick=${downloadCodes}>${t('totp.codes.download')}</button>
      </div>
      <${Checkbox}
        label=${t('totp.codes.saved_label')}
        checked=${confirmed}
        onChange=${(/** @type {boolean} */ v) => onConfirm(v)}
      />
      <div class="flex justify-end mt-2">
        <button type="button" class="btn-primary btn-sm" disabled=${!confirmed} onClick=${onDone}>${t('totp.action.done')}</button>
      </div>
    </div>
  `;
}
