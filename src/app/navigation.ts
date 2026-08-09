import { Bot, FolderGit2, ListTodo, ScrollText } from "lucide-react";
import { ProjectsView } from "../features/projects";
import { ProvidersView } from "../features/providers";
import { TasksView } from "../features/tasks";
import { TimelineView } from "../features/timeline";

export const destinations = [
  { name: "Projects", icon: FolderGit2, view: ProjectsView },
  { name: "Timeline", icon: ScrollText, view: TimelineView },
  { name: "Tasks", icon: ListTodo, view: TasksView },
  { name: "Providers", icon: Bot, view: ProvidersView },
] as const;

export type DestinationName = (typeof destinations)[number]["name"];
