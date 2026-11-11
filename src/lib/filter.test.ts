import { describe, expect, it } from "vitest";
import { filterItems, listAction, matches, moveSelection } from "./filter";

const items = [
  { repo: "octo-org/web-app", actor: "sam", text: "Looks good overall", title: "Speed up the build cache" },
  { repo: "octo-org/api-server", actor: "alex", text: "@octocat can you look?", title: "Flaky login test" },
  { repo: "octo-org/web-app", actor: "jordan", text: "Good point", title: "Rework settings" },
];

describe("matches", () => {
  it("matches every word across repo, actor and text, case-insensitively", () => {
    expect(matches(items[0], "WEB sam")).toBe(true);
    expect(matches(items[0], "web alex")).toBe(false);
    expect(matches(items[1], "flaky")).toBe(true);
    expect(matches(items[2], "")).toBe(true);
  });
  it("filters lists", () => {
    expect(filterItems(items, "web-app").map((i) => i.actor)).toEqual(["sam", "jordan"]);
    expect(filterItems(items, "nothing-here")).toEqual([]);
  });
});

describe("moveSelection", () => {
  it("clamps to the list and starts from the ends", () => {
    expect(moveSelection(-1, 1, 3)).toBe(0);
    expect(moveSelection(-1, -1, 3)).toBe(2);
    expect(moveSelection(0, -1, 3)).toBe(0);
    expect(moveSelection(2, 1, 3)).toBe(2);
    expect(moveSelection(1, 1, 3)).toBe(2);
    expect(moveSelection(0, 1, 0)).toBe(-1);
  });
});

describe("listAction", () => {
  it("maps keys to actions", () => {
    expect(listAction("Enter", false)).toBe("open");
    expect(listAction("Enter", true)).toBe("done");
    expect(listAction("Escape", false)).toBe("close");
    expect(listAction("ArrowDown", false)).toBe("down");
    expect(listAction("a", false)).toBeNull();
  });
});
