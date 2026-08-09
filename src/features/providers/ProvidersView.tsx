import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Pencil, Plus, Terminal, Trash2 } from "lucide-react";
import { createProvider, listProviders, removeProvider, updateProvider } from "./api";
import type { GenericProfile } from "./model";

const emptyProfile = (): GenericProfile => ({ id: "", displayName: "", executablePath: "", arguments: ["{prompt}"], promptMode: "argument", configRootEnvVar: null, configSourcePath: null });

export function ProvidersView() {
  const [profiles, setProfiles] = useState<GenericProfile[]>([]);
  const [draft, setDraft] = useState<GenericProfile | null>(null);
  const [error, setError] = useState("");
  useEffect(() => { listProviders().then(setProfiles).catch(() => undefined); }, []);

  async function selectExecutable() { const path = await open({ multiple: false, title: "Choose CLI executable" }); if (path) setDraft((value) => value && ({ ...value, executablePath: path })); }
  async function selectConfig() { const path = await open({ directory: true, multiple: false, title: "Choose optional config template" }); if (path) setDraft((value) => value && ({ ...value, configSourcePath: path })); }
  async function save() { if (!draft) return; setError(""); try { const saved = draft.id ? await updateProvider(draft) : await createProvider(draft); setProfiles((items) => [...items.filter((item) => item.id !== saved.id), saved]); setDraft(null); } catch (reason) { setError(String(reason)); } }
  async function remove(id: string) { await removeProvider(id); setProfiles((items) => items.filter((item) => item.id !== id)); }

  return <div className="w-full p-7">
    <div className="flex h-11 items-center justify-between border-b border-line"><h1 className="text-[15px] font-medium">Generic CLI profiles</h1><button className="button-primary" onClick={() => setDraft(emptyProfile())} type="button"><Plus size={14} />Add profile</button></div>
    <p className="my-4 max-w-3xl text-sm leading-6 text-secondary">Profiles launch any local coding CLI with direct arguments, an isolated config folder, and no shell expansion.</p>
    {error && <p className="error-banner" role="alert">{error}</p>}
    {draft && <section className="form-panel" aria-label={draft.id ? "Edit generic CLI profile" : "New generic CLI profile"}>
      <label>Name<input value={draft.displayName} onChange={(event) => setDraft({ ...draft, displayName: event.target.value })} /></label>
      <label>Executable<div className="input-action"><input readOnly value={draft.executablePath} /><button onClick={selectExecutable} type="button">Choose…</button></div></label>
      <label>Prompt delivery<select value={draft.promptMode} onChange={(event) => setDraft({ ...draft, promptMode: event.target.value as GenericProfile["promptMode"], arguments: event.target.value === "argument" ? ["{prompt}"] : [] })}><option value="argument">Argument token</option><option value="stdin">Standard input</option></select></label>
      <fieldset><legend>Arguments (one token per row)</legend>{draft.arguments.map((argument, index) => <div className="input-action" key={index}><input aria-label={`Argument ${index + 1}`} value={argument} onChange={(event) => setDraft({ ...draft, arguments: draft.arguments.map((item, itemIndex) => itemIndex === index ? event.target.value : item) })} /><button aria-label={`Remove argument ${index + 1}`} onClick={() => setDraft({ ...draft, arguments: draft.arguments.filter((_, itemIndex) => itemIndex !== index) })} type="button"><Trash2 size={13} /></button></div>)}<button className="button-secondary mt-2" onClick={() => setDraft({ ...draft, arguments: [...draft.arguments, ""] })} type="button">Add argument</button></fieldset>
      <label>Config environment variable<input placeholder="Example: AGENT_CONFIG_HOME" value={draft.configRootEnvVar ?? ""} onChange={(event) => setDraft({ ...draft, configRootEnvVar: event.target.value || null })} /></label>
      <label>Config template<div className="input-action"><input readOnly value={draft.configSourcePath ?? "Managed empty folder"} /><button onClick={selectConfig} type="button">Choose…</button></div></label>
      <div className="flex gap-2"><button className="button-primary" onClick={save} type="button">Save profile</button><button className="button-secondary" onClick={() => setDraft(null)} type="button">Cancel</button></div>
    </section>}
    <div className="border-t border-line">{profiles.length ? profiles.map((profile) => <div className="flex min-h-16 items-center gap-3 border-b border-line px-3" key={profile.id}><Terminal size={16} /><span className="min-w-0 flex-1"><strong className="block text-sm">{profile.displayName}</strong><small className="block truncate text-tertiary">{profile.executablePath}</small></span><span className="status-pill">Ready</span><button className="icon-button" aria-label={`Edit ${profile.displayName}`} onClick={() => setDraft(profile)} type="button"><Pencil size={14} /></button><button className="icon-button" aria-label={`Remove ${profile.displayName}`} onClick={() => remove(profile.id)} type="button"><Trash2 size={14} /></button></div>) : <p className="empty-row">No CLI profiles configured.</p>}</div>
  </div>;
}
