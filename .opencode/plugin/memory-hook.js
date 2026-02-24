import * as fs from "node:fs";
import * as path from "node:path";

const SKILL_LOCATIONS = [
  (dir) => path.join(dir, ".opencode", "skill", "project-memory", "SKILL.md"),
  () =>
    path.join(
      process.env.HOME || process.env.USERPROFILE || "",
      ".config",
      "opencode",
      "skill",
      "project-memory",
      "SKILL.md"
    ),
  () =>
    path.join(
      process.env.HOME || process.env.USERPROFILE || "",
      ".claude",
      "skills",
      "project-memory",
      "SKILL.md"
    ),
];

function hasMemorySkill(projectDir) {
  return SKILL_LOCATIONS.some((loc) => {
    try {
      return fs.existsSync(loc(projectDir));
    } catch {
      return false;
    }
  });
}

export const MemoryHook = async (ctx) => {
  const loadedSessions = new Set();
  const subagentSessions = new Set();

  async function isSubagent(sessionID) {
    if (subagentSessions.has(sessionID)) return true;
    try {
      const session = await ctx.client.session.get({ path: { id: sessionID } });
      if (session.data?.parentID) {
        subagentSessions.add(sessionID);
        return true;
      }
    } catch {
      return false;
    }
    return false;
  }

  return {
    "chat.message": async (input, output) => {
      if (!hasMemorySkill(ctx.directory)) return;
      if (loadedSessions.has(input.sessionID)) return;
      loadedSessions.add(input.sessionID);
      if (await isSubagent(input.sessionID)) return;

      const textPart = output.parts.find((p) => p.type === "text");
      if (textPart?.text) {
        textPart.text = `<system-reminder>Load project memory: read .memory/SUMMARY.md before starting work. If .memory/ doesn't exist, initialize it per the project-memory skill.</system-reminder>\n\n${textPart.text}`;
      }
    },

    "tool.execute.after": async (input, output) => {
      if (ctx.client.app?.log) {
        await ctx.client.app.log({
          body: {
            service: "memory-hook",
            level: "debug",
            message: `tool.execute.after fired: tool="${input.tool}"`,
          },
          query: { directory: ctx.directory },
        });
      }

      const toolLower = (input.tool || "").toLowerCase().replace("mcp_", "");
      if (toolLower !== "todowrite") return;
      if (await isSubagent(input.sessionID)) return;

      const todos = input.args?.todos;
      if (!Array.isArray(todos)) return;
      if (!todos.some((t) => t.status === "completed")) return;

      output.output += `\n\n<system-reminder>Task completed. If this work produced knowledge worth remembering (re-discovering would cost meaningful time), capture it to .memory/ following the project-memory skill. Only save decisions, patterns, bug root causes, or preferences — skip routine changes.</system-reminder>`;
    },

    "event": async ({ event }) => {
      if (
        event.type === "session.deleted" ||
        event.type === "session.compacted"
      ) {
        const sid =
          event.properties?.info?.id || event.properties?.sessionID;
        if (sid) {
          loadedSessions.delete(sid);
          subagentSessions.delete(sid);
        }
      }
    },
  };
};
