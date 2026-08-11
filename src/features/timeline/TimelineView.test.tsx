import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { acknowledgeAttention, decideApproval, listApprovals, listAttention } from "../attention";
import { listTimeline } from "./api";
import { TimelineView } from "./TimelineView";

vi.mock("../attention", () => ({ acknowledgeAttention:vi.fn(),claimAttentionNotification:vi.fn(),decideApproval:vi.fn(),listApprovals:vi.fn(),listAttention:vi.fn() }));
vi.mock("./api", () => ({ listTimeline:vi.fn() }));

beforeEach(() => {
  vi.mocked(listTimeline).mockResolvedValue([{ id:"e1",projectId:"p",projectName:"Repo",taskId:"t",runId:"r",providerId:"provider",sequence:4,eventType:"run.status_changed",payload:{to:"failed"},createdAt:"now" }]);
  vi.mocked(listAttention).mockResolvedValue([{ key:"approval:a",reason:"approval_waiting",projectId:"p",taskId:"t",runId:"r",approvalRequestId:"a",title:"Fix",detail:"Agent requested: merge_task",stateFingerprint:"pending:now",acknowledged:false,createdAt:"now" }]);
  vi.mocked(listApprovals).mockResolvedValue([{ id:"a",projectId:"p",taskId:"t",runId:"r",action:"merge_task",arguments:{},status:"pending",requestedBy:"agent",createdAt:"now",decidedAt:null }]);
  vi.mocked(acknowledgeAttention).mockResolvedValue(undefined);
  vi.mocked(decideApproval).mockResolvedValue({ id:"a",projectId:"p",taskId:"t",runId:"r",action:"merge_task",arguments:{},status:"approved",requestedBy:"agent",createdAt:"now",decidedAt:"later" });
});

it("renders ordered activity and routes exact attention decisions", async () => {
  const open = vi.fn();
  render(<TimelineView onOpen={open} project={{ id:"p",name:"Repo",path:"/tmp/repo",lastOpenedAt:"now",git:{isRepository:true,branch:"main",revision:"abc",dirty:false} }} tasks={[{ id:"t",projectId:"p",title:"Fix",description:"",status:"working",baseBranch:"main",baseRevision:"abc",acceptanceCriteria:[],allowedPaths:[],validationCommands:[],decisions:[],updatedAt:"now" }]} />);
  expect(await screen.findByText("Agent failed")).toBeTruthy();
  const timeline = screen.getByRole("region", { name: "Project timeline" });
  fireEvent.click(within(timeline).getByRole("button", { name:"Open Fix" }));
  expect(open).toHaveBeenCalledWith("t", "r");
  fireEvent.click(screen.getByRole("button", { name:"Approve" }));
  await waitFor(() => expect(decideApproval).toHaveBeenCalledWith("a", "approved"));
});
