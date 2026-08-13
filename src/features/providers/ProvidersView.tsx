import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Check, KeyRound, Link2, LogOut, Pencil, Plus, Square, Trash2 } from "lucide-react";
import { errorMessage } from "../../shared/error";
import { createProvider, detectProviders, getDefaultProvider, listProviders, loginCodex, logoutCodex, reauthenticateProvider, removeProvider, setDefaultProvider, stopCodexLogin, updateProvider } from "./api";
import type { DetectedProvider, GenericProfile } from "./model";
import { ProviderIcon } from "./ProviderIcon";

const emptyProfile = (): GenericProfile => ({ id: "", displayName: "", providerType: "generic", status: "active", executablePath: "", arguments: ["{prompt}"], resumeArguments: [], promptMode: "argument", configRootEnvVar: null, configSourcePath: null, inheritUserHome: false });

export function ProvidersView() {
  const [profiles, setProfiles] = useState<GenericProfile[]>([]);
  const [detected, setDetected] = useState<DetectedProvider[]>([]);
  const [draft, setDraft] = useState<GenericProfile | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);
  const [credentialFor, setCredentialFor] = useState<GenericProfile | null>(null);
  const [credential, setCredential] = useState("");
  const [accountName, setAccountName] = useState("");
  const [linkFormOpen, setLinkFormOpen] = useState(false);
  const [authSession, setAuthSession] = useState<{ accountId: string; accountName: string; method: "browser" | "device"; active: boolean; output: string } | null>(null);
  const [authBusy, setAuthBusy] = useState(false);
  const [defaultId, setDefaultId] = useState<string | null>(null);

  useEffect(() => {
    Promise.all([listProviders(), detectProviders(), getDefaultProvider()])
      .then(async ([saved, installed, preferred]) => {
        const existingCodex = installed.find((provider) => provider.key === "codex" && provider.isAuthenticated);
        const hasExistingCodexProfile = saved.some((profile) => profile.providerType === "codex" && profile.inheritUserHome);
        if (existingCodex && !hasExistingCodexProfile) {
          try {
            const imported = await createProvider({ id: "", displayName: "Codex", providerType: "codex", status: "active", executablePath: existingCodex.executablePath, arguments: existingCodex.arguments, resumeArguments: existingCodex.resumeArguments, promptMode: existingCodex.promptMode, configRootEnvVar: existingCodex.configRootEnvVar, configSourcePath: null, inheritUserHome: true });
            saved = [...saved, imported];
            preferred ??= imported.id;
            installed = installed.map((provider) => provider.key === "codex" ? { ...provider, isConfigured: true } : provider);
          } catch (reason) {
            setError(message(reason));
          }
        }
        setProfiles(saved);
        setDetected(installed);
        setDefaultId(preferred);
      })
      .catch((reason) => setError(message(reason)))
      .finally(() => setLoading(false));
  }, []);
  useEffect(() => {
    if (!authSession?.active) return;
    let disposed = false;
    const refresh = () => listProviders().then((saved) => {
      if (disposed) return;
      const linked = saved.find((profile) => profile.id === authSession.accountId && profile.status === "active");
      if (!linked) return;
      setProfiles(saved);
      setAuthSession((current) => current?.accountId === linked.id ? { ...current, active: false, output: `${current.output}\nCodex account linked` } : current);
      void getDefaultProvider().then((preferred) => !disposed && setDefaultId(preferred));
    }).catch(() => undefined);
    refresh();
    const timer = window.setInterval(refresh, 750);
    return () => { disposed = true; window.clearInterval(timer); };
  }, [authSession?.accountId, authSession?.active]);

  async function useDetected(provider: DetectedProvider) {
    setError("");
    try {
      const saved = await createProvider({ id: "", displayName: provider.displayName, providerType: provider.key, status: "active", executablePath: provider.executablePath, arguments: provider.arguments, resumeArguments: provider.resumeArguments, promptMode: provider.promptMode, configRootEnvVar: provider.configRootEnvVar, configSourcePath: null, inheritUserHome: true });
      setProfiles((items) => [...items.filter((item) => item.executablePath !== saved.executablePath), saved]);
      setDetected((items) => items.map((item) => item.key === provider.key ? { ...item, isConfigured: true } : item));
    } catch (reason) { setError(message(reason)); }
  }
  async function selectExecutable() { const path = await open({ multiple: false, title: "Choose CLI executable" }); if (path) setDraft((value) => value && ({ ...value, executablePath: path })); }
  async function selectConfig() { const path = await open({ directory: true, multiple: false, title: "Choose optional config template" }); if (path) setDraft((value) => value && ({ ...value, configSourcePath: path })); }
  async function save() { if (!draft) return; setError(""); try { const saved = draft.id ? await updateProvider(draft) : await createProvider(draft); setProfiles((items) => [...items.filter((item) => item.id !== saved.id), saved]); setDraft(null); } catch (reason) { setError(message(reason)); } }
  async function remove(id: string) { const profile = profiles.find((item) => item.id === id); if (!window.confirm(`Remove ${profile?.displayName ?? "this provider"} and its saved credential?`)) return; setError(""); try { await removeProvider(id); const removed = profiles.find((item) => item.id === id); setProfiles((items) => items.filter((item) => item.id !== id)); if (id === defaultId) setDefaultId(await getDefaultProvider()); if (removed) setDetected((items) => items.map((item) => item.executablePath === removed.executablePath ? { ...item, isConfigured: false } : item)); } catch (reason) { setError(message(reason)); } }
  async function reauthenticate() { if (!credentialFor || !credential) return; setError(""); try { const saved = await reauthenticateProvider(credentialFor.id, credential); setProfiles((items) => items.map((item) => item.id === saved.id ? saved : item)); setCredential(""); setCredentialFor(null); } catch (reason) { setError(message(reason)); } }
  function startCodexLogin(profile: GenericProfile, method: "browser" | "device") { setError(""); setAuthSession({ accountId: profile.id, accountName: profile.displayName, method, active: true, output: method === "device" ? "Starting device-code sign-in…\n" : "Opening ChatGPT sign-in…\n" }); return loginCodex(profile.id, method, (event) => { if (event.type === "output") setAuthSession((current) => current?.accountId === profile.id ? { ...current, output: cleanTerminalOutput(`${current.output}${event.text}`).slice(-65536) } : current); else { if (event.account) setProfiles((items) => items.map((item) => item.id === event.account!.id ? event.account! : item)); setAuthSession((current) => current?.accountId === profile.id ? { ...current, active: false, output: cleanTerminalOutput(`${current.output}\n${event.message}`) } : current); if (!event.success) setError(event.message); } }); }
  async function linkCodex(method: "browser" | "device") { const codex = detected.find((provider) => provider.key === "codex"); if (!codex || !accountName.trim()) return; setAuthBusy(true); setError(""); try { const saved = await createProvider({ id: "", displayName: accountName.trim(), providerType: "codex", status: "needs_reauth", executablePath: codex.executablePath, arguments: codex.arguments, resumeArguments: codex.resumeArguments, promptMode: codex.promptMode, configRootEnvVar: codex.configRootEnvVar, configSourcePath: null, inheritUserHome: false }); setProfiles((items) => [...items.filter((item) => item.id !== saved.id), saved]); setAccountName(""); setLinkFormOpen(false); await startCodexLogin(saved, method); } catch (reason) { setError(message(reason)); } finally { setAuthBusy(false); } }
  async function signOut(profile: GenericProfile) { if (!window.confirm(`Sign out ${profile.displayName}? Active runs must be stopped first.`)) return; setError(""); try { const saved = await logoutCodex(profile.id); setProfiles((items) => items.map((item) => item.id === saved.id ? saved : item)); if (profile.id === defaultId) setDefaultId(await getDefaultProvider()); } catch (reason) { setError(message(reason)); } }
  async function useForNewGoals(profile: GenericProfile) { setError(""); try { setDefaultId(await setDefaultProvider(profile.id)); } catch (reason) { setError(message(reason)); } }
  const codex = detected.find((provider) => provider.key === "codex");
  const codexAccounts = profiles.filter((profile) => profile.providerType === "codex");
  const otherProfiles = profiles.filter((profile) => profile.providerType !== "codex");
  const authLink = authSession?.output.match(/https:\/\/[^\s\x1b]+/)?.[0] ?? null;
  const authCode = authSession?.output.match(/\b[A-Z0-9]{4,6}-[A-Z0-9]{4,6}\b/)?.[0] ?? null;

  return <div className="w-full p-7">
    <div className="flex min-h-11 flex-wrap items-center justify-between gap-2 border-b border-line"><h1 className="text-[15px] font-medium">Provider accounts</h1><div className="flex items-center gap-2"><span className="status-pill">Account linking · Codex only</span><button className="button-secondary" onClick={() => setDraft(emptyProfile())} type="button"><Plus size={14} />Custom CLI</button></div></div>
    <p className="my-4 max-w-3xl text-sm leading-6 text-secondary">Link separate ChatGPT accounts for Codex. Every account gets its own Codex home and OS-keychain entry. New goals use the account marked Default; individual assignments can choose any linked account.</p>
    {error && <p className="error-banner" role="alert">{error}</p>}

    <section aria-labelledby="codex-accounts"><div className="flex flex-wrap items-center justify-between gap-2 border-b border-line px-3 py-2.5"><div><h2 className="table-label" id="codex-accounts">Codex accounts</h2><p className="mt-1 text-[11px] text-tertiary">Use normal ChatGPT sign-in or a device code. Credentials stay in your OS keychain.</p></div><button className="button-primary" disabled={!codex} onClick={() => setLinkFormOpen(true)} type="button"><Link2 size={14} />Link account</button></div>
      {!codex && !loading && <p className="empty-row">Install the Codex CLI first, then return here to link a ChatGPT account.</p>}
      {codexAccounts.map((profile) => <div className="flex min-h-16 flex-wrap items-center gap-3 border-b border-line px-3 py-2" key={profile.id}><ProviderIcon name="Codex" /><span className="min-w-44 flex-1"><strong className="block text-sm">{profile.displayName}</strong><small className="block text-tertiary">{profile.inheritUserHome ? "ChatGPT · Existing Codex home" : "ChatGPT · Separate Codex home"}</small></span><span className="status-pill">{profile.status === "active" ? profile.id === defaultId ? "Default" : profile.inheritUserHome ? "Detected" : "Linked" : "Needs sign-in"}</span>{profile.status === "active" && profile.id !== defaultId && <button className="button-secondary" onClick={() => useForNewGoals(profile)} type="button">Use for new goals</button>}{!profile.inheritUserHome && <><button className="button-secondary" disabled={authSession?.active && authSession.accountId === profile.id} onClick={() => startCodexLogin(profile, "browser").catch((reason) => setError(message(reason)))} type="button"><Link2 size={13} />{profile.status === "active" ? "Relink" : "Sign in"}</button><button className="button-secondary" disabled={authSession?.active && authSession.accountId === profile.id} onClick={() => startCodexLogin(profile, "device").catch((reason) => setError(message(reason)))} type="button">Device code</button>{profile.status === "active" && <button aria-label={`Sign out ${profile.displayName}`} className="icon-button" onClick={() => signOut(profile)} type="button"><LogOut size={14} /></button>}</>}<button aria-label={`Remove ${profile.displayName}`} className="icon-button" onClick={() => remove(profile.id)} type="button"><Trash2 size={14} /></button></div>)}
      {codex && !codexAccounts.length && !linkFormOpen && <p className="empty-row">No Codex accounts linked yet. Add one for each ChatGPT email you want to use.</p>}
    </section>

    {linkFormOpen && codex && <section className="form-panel mt-4" aria-label="Link Codex account"><div><h2 className="text-sm font-medium">Link a ChatGPT account</h2><p className="mt-1 text-xs text-tertiary">Give it a recognizable label. You will choose the email in ChatGPT&apos;s sign-in flow.</p></div><label>Account label<input autoFocus onChange={(event) => setAccountName(event.target.value)} placeholder="Work email" value={accountName} /></label><div className="flex flex-wrap gap-2"><button className="button-primary" disabled={!accountName.trim() || authBusy} onClick={() => linkCodex("browser")} type="button">Continue with ChatGPT</button><button className="button-secondary" disabled={!accountName.trim() || authBusy} onClick={() => linkCodex("device")} type="button">Use device code</button><button className="button-secondary" onClick={() => { setLinkFormOpen(false); setAccountName(""); }} type="button">Cancel</button></div><p className="text-xs leading-5 text-tertiary">Device code is useful when the browser callback is blocked. It must be enabled in your ChatGPT security or workspace settings.</p></section>}

    {authSession && <section className="mt-4 border-y border-line px-3 py-4" aria-label={`Sign in ${authSession.accountName}`}><div className="flex flex-wrap items-start justify-between gap-3"><div><h2 className="text-sm font-medium">{authSession.active ? authSession.method === "device" ? "Enter the code in ChatGPT" : "Finish signing in with ChatGPT" : "Sign-in finished"}</h2><p className="mt-1 text-xs text-tertiary">{authSession.method === "device" ? `Link ${authSession.accountName} with the one-time code below.` : `Complete the browser sign-in for ${authSession.accountName}.`} SubShell verifies it automatically.</p></div><span className="status-pill">{authSession.active ? "Waiting" : "Finished"}</span></div><div className="mt-3 flex flex-wrap items-center gap-2">{authCode && <code className="select-all rounded border border-line-strong bg-app px-3 py-2 font-mono text-sm font-semibold tracking-widest text-primary">{authCode}</code>}{authLink && <a className="button-primary" href={authLink} rel="noreferrer" target="_blank">Continue to ChatGPT</a>}{authSession.active && <button className="button-secondary" onClick={() => stopCodexLogin(authSession.accountId).catch((reason) => setError(message(reason)))} type="button"><Square size={12} />Cancel</button>}</div><details className="mt-3 text-xs text-tertiary"><summary className="w-fit cursor-pointer select-none hover:text-secondary">Sign-in details</summary><pre aria-live="polite" className="mt-2 max-h-24 overflow-auto whitespace-pre-wrap font-mono text-[11px] leading-5 text-tertiary" role="log">{authSession.output}</pre></details></section>}

    <section aria-labelledby="detected-agents"><h2 className="table-label border-b border-line px-3 py-2.5" id="detected-agents">Detected on this computer</h2>
      {loading ? <p className="empty-row" role="status">Looking for installed agents…</p> : detected.length ? detected.map((provider) => <div className="flex min-h-16 items-center gap-3 border-b border-line px-3" key={provider.key}><span className="icon-box"><ProviderIcon name={provider.displayName} /></span><span className="min-w-0 flex-1"><strong className="block text-sm font-medium">{provider.displayName}</strong><small className="block truncate font-mono text-[11px] text-tertiary">{provider.executablePath}</small></span>{provider.key === "codex" ? <span className="text-xs text-secondary">{provider.isAuthenticated ? "Existing login detected" : "Link accounts above"}</span> : provider.isConfigured ? <span className="flex items-center gap-1.5 text-xs text-secondary"><Check aria-hidden="true" size={14} />Ready</span> : <button className="button-primary" onClick={() => useDetected(provider)} type="button">Configure</button>}</div>) : <p className="empty-row">No supported CLI was found. Install Claude Code, Codex, Kiro, or Gemini, or choose Custom CLI.</p>}
    </section>

    {otherProfiles.length > 0 && <section className="mt-6" aria-labelledby="configured-agents"><h2 className="table-label border-b border-line px-3 py-2.5" id="configured-agents">Other CLI profiles</h2>{otherProfiles.map((profile) => <div className="flex min-h-16 flex-wrap items-center gap-3 border-b border-line px-3 py-2" key={profile.id}><ProviderIcon name={profile.displayName} /><span className="min-w-0 flex-1"><strong className="block text-sm">{profile.displayName}</strong><small className="block truncate text-tertiary">{profile.inheritUserHome ? "Using existing CLI login" : "Isolated configuration"}</small></span><span className="status-pill">{profile.status === "needs_reauth" ? "Needs reauth" : profile.status === "revoked" ? "Revoked" : profile.id === defaultId ? "Default" : "Ready"}</span>{profile.status === "active" && profile.id !== defaultId && <button className="button-secondary" onClick={() => useForNewGoals(profile)} type="button">Use for new goals</button>}<button className="icon-button" aria-label={`Update credential for ${profile.displayName}`} onClick={() => { setCredentialFor(profile); setCredential(""); }} type="button"><KeyRound size={14} /></button><button className="icon-button" aria-label={`Edit ${profile.displayName}`} onClick={() => setDraft(profile)} type="button"><Pencil size={14} /></button><button className="icon-button" aria-label={`Remove ${profile.displayName}`} onClick={() => remove(profile.id)} type="button"><Trash2 size={14} /></button></div>)}</section>}

    {credentialFor && <section className="form-panel mt-6" aria-label={`Reauthenticate ${credentialFor.displayName}`}><div><h2 className="text-sm font-medium text-primary">Update {credentialFor.displayName} credential</h2><p className="mt-1 text-xs text-tertiary">Stored only in your operating system keychain.</p></div><label>API token<input autoComplete="off" onChange={(event) => setCredential(event.target.value)} type="password" value={credential} /></label><div className="flex gap-2"><button className="button-primary" disabled={!credential} onClick={reauthenticate} type="button">Save credential</button><button className="button-secondary" onClick={() => { setCredential(""); setCredentialFor(null); }} type="button">Cancel</button></div></section>}

    {draft && <section className="form-panel mt-6" aria-label={draft.id ? "Edit custom CLI" : "Add custom CLI"}>
      <div><h2 className="text-sm font-medium text-primary">Advanced CLI setup</h2><p className="mt-1 text-xs text-tertiary">Only needed when your agent was not detected automatically.</p></div>
      <label>Name<input value={draft.displayName} onChange={(event) => setDraft({ ...draft, displayName: event.target.value })} /></label>
      <label>Executable<div className="input-action"><input readOnly value={draft.executablePath} /><button onClick={selectExecutable} type="button">Choose…</button></div></label>
      <label>Prompt delivery<select value={draft.promptMode} onChange={(event) => setDraft({ ...draft, promptMode: event.target.value as GenericProfile["promptMode"], arguments: event.target.value === "argument" ? ["{prompt}"] : [] })}><option value="argument">Argument token</option><option value="stdin">Standard input</option></select></label>
      <fieldset><legend>Arguments (one token per row)</legend>{draft.arguments.map((argument, index) => <div className="input-action" key={index}><input aria-label={`Argument ${index + 1}`} value={argument} onChange={(event) => setDraft({ ...draft, arguments: draft.arguments.map((item, itemIndex) => itemIndex === index ? event.target.value : item) })} /><button aria-label={`Remove argument ${index + 1}`} onClick={() => setDraft({ ...draft, arguments: draft.arguments.filter((_, itemIndex) => itemIndex !== index) })} type="button"><Trash2 size={13} /></button></div>)}<button className="button-secondary mt-2" onClick={() => setDraft({ ...draft, arguments: [...draft.arguments, ""] })} type="button">Add argument</button></fieldset>
      <label className="check-row"><input checked={draft.inheritUserHome} onChange={(event) => setDraft({ ...draft, inheritUserHome: event.target.checked })} type="checkbox" />Use this CLI&apos;s existing login and home configuration</label>
      {!draft.inheritUserHome && <><label>Config environment variable<input placeholder="Example: AGENT_CONFIG_HOME" value={draft.configRootEnvVar ?? ""} onChange={(event) => setDraft({ ...draft, configRootEnvVar: event.target.value || null })} /></label><label>Config template<div className="input-action"><input readOnly value={draft.configSourcePath ?? "Managed empty folder"} /><button onClick={selectConfig} type="button">Choose…</button></div></label></>}
      <div className="flex gap-2"><button className="button-primary" onClick={save} type="button">Save CLI</button><button className="button-secondary" onClick={() => setDraft(null)} type="button">Cancel</button></div>
    </section>}
  </div>;
}

function message(error: unknown) { return errorMessage(error, "The provider could not be configured."); }
function cleanTerminalOutput(value: string) { return value.replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, ""); }
