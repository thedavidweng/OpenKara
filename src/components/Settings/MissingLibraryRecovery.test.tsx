// @vitest-environment jsdom
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vite-plus/test";
import { createMockBackend } from "@/lib/backend/mock-backend";
import type { LibraryRegistrySnapshot, RegisteredLibrary } from "@/types/ipc";
import { MissingLibraryRecovery } from "./MissingLibraryRecovery";

const mockOpen = vi.hoisted(() => vi.fn());
const session = vi.hoisted(() => ({
  switchLibrary: vi.fn(),
  openLocalLibrary: vi.fn(),
  createLocalLibrary: vi.fn(),
}));

vi.mock("react-i18next", async (importOriginal) => ({
  ...(await importOriginal<typeof import("react-i18next")>()),
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: mockOpen }));
vi.mock("@/lib/library-session", () => ({
  createLibrarySession: () => session,
}));

const getLibraryRegistry = vi.fn();
const removeLibrary = vi.fn();
const backend = createMockBackend({
  overrides: { librarySetup: { getLibraryRegistry, removeLibrary } },
});
vi.mock("@/lib/backend", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/backend")>()),
  useBackend: () => backend,
}));

const library = (id: string, name: string) =>
  ({
    kind: "local",
    id,
    display_name: name,
    path: `/libraries/${id}`,
  }) as unknown as RegisteredLibrary;

const registry: LibraryRegistrySnapshot = {
  active_library_id: "gone",
  libraries: [library("gone", "Gone"), library("backup", "Backup Library")],
  active_library_unavailable: {
    path: "/Users/demo/Music/OpenKara",
    reason: "not_found",
  },
};

const onResolved = vi.fn();
const show = (snapshot = registry) =>
  render(
    <MissingLibraryRecovery registry={snapshot} onResolved={onResolved} />,
  );
const click = (name: string) =>
  fireEvent.click(screen.getByRole("button", { name: new RegExp(name) }));

beforeEach(() => {
  for (const fn of [
    mockOpen,
    onResolved,
    getLibraryRegistry,
    removeLibrary,
    ...Object.values(session),
  ])
    fn.mockReset();
});
afterEach(cleanup);

test("names the missing path and lists only the other libraries", () => {
  show();
  expect(screen.getByText("/Users/demo/Music/OpenKara")).toBeTruthy();
  expect(screen.getByText("libraryRecovery.descriptionNotFound")).toBeTruthy();
  expect(screen.getByRole("button", { name: /Backup Library/ })).toBeTruthy();
  expect(screen.queryByRole("button", { name: /^Gone$/ })).toBeNull();
});

test("explains a folder that exists but cannot be opened", () => {
  show({
    ...registry,
    active_library_unavailable: {
      path: "/x",
      reason: "not_openable",
    },
  });
  expect(
    screen.getByText("libraryRecovery.descriptionNotOpenable"),
  ).toBeTruthy();
});

test("renders nothing when the active library is available", () => {
  const { container } = show({
    ...registry,
    active_library_unavailable: null,
  });
  expect(container.innerHTML).toBe("");
});

test("retry switches to the missing library and resolves", async () => {
  session.switchLibrary.mockResolvedValue(undefined);
  show();
  click("libraryRecovery.retry");
  await waitFor(() => expect(onResolved).toHaveBeenCalled());
  expect(session.switchLibrary).toHaveBeenCalledWith("gone");
});

test("a failed retry shows the error and stays", async () => {
  session.switchLibrary.mockRejectedValue(new Error("still missing"));
  show();
  click("libraryRecovery.retry");
  expect((await screen.findByRole("alert")).textContent).toContain(
    "still missing",
  );
  expect(onResolved).not.toHaveBeenCalled();
});

test("another library can be opened instead", async () => {
  session.switchLibrary.mockResolvedValue(undefined);
  show();
  click("Backup Library");
  await waitFor(() => expect(onResolved).toHaveBeenCalled());
  expect(session.switchLibrary).toHaveBeenCalledWith("backup");
});

test("locate opens the chosen folder and removes the stale entry", async () => {
  mockOpen.mockResolvedValue("/Volumes/Music/OpenKara");
  session.openLocalLibrary.mockResolvedValue(undefined);
  getLibraryRegistry.mockResolvedValue({
    ...registry,
    active_library_id: "moved",
  });
  show();
  click("libraryRecovery.locate");
  await waitFor(() => expect(onResolved).toHaveBeenCalled());
  expect(session.openLocalLibrary).toHaveBeenCalledWith(
    "/Volumes/Music/OpenKara",
  );
  expect(removeLibrary).toHaveBeenCalledWith("gone");
});

test("cancelling the folder picker does nothing", async () => {
  mockOpen.mockResolvedValue(null);
  show();
  click("libraryRecovery.locate");
  click("setup.createNew");
  await waitFor(() => expect(mockOpen).toHaveBeenCalledTimes(2));
  expect(session.openLocalLibrary).not.toHaveBeenCalled();
  expect(session.createLocalLibrary).not.toHaveBeenCalled();
  expect(onResolved).not.toHaveBeenCalled();
});

test("create makes a new library in the chosen folder", async () => {
  mockOpen.mockResolvedValue(["/Users/demo/Karaoke"]);
  session.createLocalLibrary.mockResolvedValue(undefined);
  show();
  click("setup.createNew");
  await waitFor(() => expect(onResolved).toHaveBeenCalled());
  expect(session.createLocalLibrary).toHaveBeenCalledWith(
    "/Users/demo/Karaoke",
  );
});
