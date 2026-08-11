import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Check, KeyRound, Pencil, Plus, Trash2 } from "lucide-react";
import { createProvider, detectProviders, listProviders, reauthenticateProvider, removeProvider, updateProvider } from "./api";
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

  useEffect(() => {
    Promise.all([listProviders(), detectProviders()])
      .then(([saved, installed]) => { setProfiles(saved); setDetected(installed); })
      .catch((reason) => setError(message(reason)))
      .finally(() => setLoading(false));
  }, []);

  async function useDetected(provider: DetectedProvider) {
    setError("");
    try {
      const saved = await createProvider({ id: "", displayName: provider.displayName, providerType: provider.key, status: "active", executablePath: provider.executablePath, arguments: provider.arguments, resumeArguments: provider.resumeArguments, promptMode: provider.promptMode, configRootEnvVar: provider.configRootEnvVar, configSourcePath: null, inheritUserHome: false });
      setProfiles((items) => [...items.filter((item) => item.executablePath !== saved.executablePath), saved]);
      setDetected((items) => items.map((item) => item.key === provider.key ? { ...item, isConfigured: true } : item));
    } catch (reason) { setError(message(reason)); }
  }
  async function selectExecutable() { const path = await open({ multiple: false, title: "Choose CLI executable" }); if (path) setDraft((value) => value && ({ ...value, executablePath: path })); }
  async function selectConfig() { const path = await open({ directory: true, multiple: false, title: "Choose optional config template" }); if (path) setDraft((value) => value && ({ ...value, configSourcePath: path })); }
  async function save() { if (!draft) return; setError(""); try { const saved = draft.id ? await updateProvider(draft) : await createProvider(draft); setProfiles((items) => [...items.filter((item) => item.id !== saved.id), saved]); setDraft(null); } catch (reason) { setError(message(reason)); } }
  async function remove(id: string) { setError(""); try { await removeProvider(id); const removed = profiles.find((profile) => profile.id === id); setProfiles((items) => items.filter((item) => item.id !== id)); if (removed) setDetected((items) => items.map((item) => item.executablePath === removed.executablePath ? { ...item, isConfigured: false } : item)); } catch (reason) { setError(message(reason)); } }
  async function reauthenticate() { if (!credentialFor || !credential) return; setError(""); try { const saved = await reauthenticateProvider(credentialFor.id, credential); setProfiles((items) => items.map((item) => item.id === saved.id ? saved : item)); setCredential(""); setCredentialFor(null); } catch (reason) { setError(message(reason)); } }

  return <div className="w-full p-7">
    <div className="flex h-11 items-center justify-between border-b border-line"><h1 className="text-[15px] font-medium">AI agents</h1><button className="button-secondary" onClick={() => setDraft(emptyProfile())} type="button"><Plus size={14} />Custom CLI</button></div>
    <p className="my-4 max-w-3xl text-sm leading-6 text-secondary">The first prompt automatically uses an installed coding agent in an isolated configuration. Use this screen to add credentials, choose another agent, or configure an unsupported CLI.</p>
    {error && <p className="error-banner" role="alert">{error}</p>}

    <section aria-labelledby="detected-agents"><h2 className="table-label border-b border-line px-3 py-2.5" id="detected-agents">Detected on this computer</h2>
      {loading ? <p className="empty-row">Looking for installed agents…</p> : detected.length ? detected.map((provider) => <div className="flex min-h-16 items-center gap-3 border-b border-line px-3" key={provider.key}><span className="icon-box"><ProviderIcon name={provider.displayName} /></span><span className="min-w-0 flex-1"><strong className="block text-sm font-medium">{provider.displayName}</strong><small className="block truncate font-mono text-[11px] text-tertiary">{provider.executablePath}</small></span>{provider.isConfigured ? <span className="flex items-center gap-1.5 text-xs text-secondary"><Check size={14} />Ready</span> : <button className="button-primary" onClick={() => useDetected(provider)} type="button">Configure</button>}</div>) : <p className="empty-row">No Claude Code, Codex, Kiro, or Gemini CLI installation was found in the app PATH.</p>}
    </section>

    {profiles.length > 0 && <section className="mt-6" aria-labelledby="configured-agents"><h2 className="table-label border-b border-line px-3 py-2.5" id="configured-agents">Configured agents</h2>{profiles.map((profile) => <div className="flex min-h-16 items-center gap-3 border-b border-line px-3" key={profile.id}><ProviderIcon name={profile.displayName} /><span className="min-w-0 flex-1"><strong className="block text-sm">{profile.displayName}</strong><small className="block truncate text-tertiary">{profile.inheritUserHome ? "Using existing CLI login" : "Isolated configuration"}</small></span><span className="status-pill">{profile.status === "needs_reauth" ? "Needs reauth" : profile.status === "revoked" ? "Revoked" : "Ready"}</span><button className="icon-button" aria-label={`Update credential for ${profile.displayName}`} onClick={() => { setCredentialFor(profile); setCredential(""); }} type="button"><KeyRound size={14} /></button><button className="icon-button" aria-label={`Edit ${profile.displayName}`} onClick={() => setDraft(profile)} type="button"><Pencil size={14} /></button><button className="icon-button" aria-label={`Remove ${profile.displayName}`} onClick={() => remove(profile.id)} type="button"><Trash2 size={14} /></button></div>)}</section>}

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

function message(error: unknown) { if (typeof error === "string") return error; if (error && typeof error === "object" && "message" in error) return String(error.message); return "The provider could not be configured."; }
