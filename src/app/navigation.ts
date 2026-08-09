import { Bot, FolderGit2, ListTodo, PlugZap, ScrollText } from "lucide-react";
import { ProjectsView } from "../features/projects";
import { ProvidersView } from "../features/providers";
import { AgentsView } from "../features/runs";
import { TasksView } from "../features/tasks";
import { TimelineView } from "../features/timeline";

export const destinations = [
  { name: "Projects", icon: FolderGit2, view: ProjectsView },
  { name: "Activity", icon: ScrollText, view: TimelineView },
  { name: "Tasks", icon: ListTodo, view: TasksView },
  { name: "Agents", icon: Bot, view: AgentsView },
  { name: "Providers", icon: PlugZap, view: ProvidersView },
] as const;

export type DestinationName = (typeof destinations)[number]["name"];
