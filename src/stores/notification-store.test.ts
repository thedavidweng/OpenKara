import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { type Notification, useNotificationStore } from "./notification-store";

let uuidCounter = 0;

function makeNotification(
  overrides: Partial<Omit<Notification, "id" | "timestamp">> = {},
): Omit<Notification, "id" | "timestamp"> {
  return {
    type: "info",
    title: "Test",
    message: "Something happened",
    retryable: false,
    dismissAfterMs: null,
    ...overrides,
  };
}

describe("notification-store", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    uuidCounter = 0;
    vi.spyOn(crypto, "randomUUID").mockImplementation(
      () => `uuid-${++uuidCounter}`,
    );
    useNotificationStore.setState({ notifications: [] });
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  test("addNotification: adds entry with id and timestamp", () => {
    const now = 1700000000000;
    vi.setSystemTime(now);

    useNotificationStore.getState().addNotification(makeNotification());

    const { notifications } = useNotificationStore.getState();
    expect(notifications).toHaveLength(1);
    expect(notifications[0]).toMatchObject({
      type: "info",
      title: "Test",
      message: "Something happened",
      retryable: false,
      dismissAfterMs: null,
    });
    expect(notifications[0].id).toBeTypeOf("string");
    expect(notifications[0].timestamp).toBe(now);
  });

  test("addNotification: caps at MAX_VISIBLE (5)", () => {
    for (let i = 1; i <= 6; i++) {
      useNotificationStore
        .getState()
        .addNotification(makeNotification({ title: `Notification ${i}` }));
    }

    const { notifications } = useNotificationStore.getState();
    expect(notifications).toHaveLength(5);
    expect(notifications[0].title).toBe("Notification 2");
    expect(notifications[4].title).toBe("Notification 6");
  });

  test("addNotification with dismissAfterMs=null: notification persists", () => {
    useNotificationStore
      .getState()
      .addNotification(makeNotification({ dismissAfterMs: null }));

    vi.advanceTimersByTime(10_000);

    expect(useNotificationStore.getState().notifications).toHaveLength(1);
  });

  test("addNotification with dismissAfterMs=100: auto-removes after 100ms", () => {
    useNotificationStore
      .getState()
      .addNotification(makeNotification({ dismissAfterMs: 100 }));

    expect(useNotificationStore.getState().notifications).toHaveLength(1);

    vi.advanceTimersByTime(99);
    expect(useNotificationStore.getState().notifications).toHaveLength(1);

    vi.advanceTimersByTime(1);
    expect(useNotificationStore.getState().notifications).toHaveLength(0);
  });

  test("dismissNotification: removes specific notification", () => {
    useNotificationStore
      .getState()
      .addNotification(makeNotification({ title: "Keep" }));
    useNotificationStore
      .getState()
      .addNotification(makeNotification({ title: "Remove" }));

    const { notifications } = useNotificationStore.getState();
    expect(notifications).toHaveLength(2);

    const idToRemove = notifications.find((n) => n.title === "Remove")!.id;
    useNotificationStore.getState().dismissNotification(idToRemove);

    const remaining = useNotificationStore.getState().notifications;
    expect(remaining).toHaveLength(1);
    expect(remaining[0].title).toBe("Keep");
  });

  test("clearAll: empties all notifications", () => {
    useNotificationStore
      .getState()
      .addNotification(makeNotification({ title: "A" }));
    useNotificationStore
      .getState()
      .addNotification(makeNotification({ title: "B" }));

    expect(useNotificationStore.getState().notifications).toHaveLength(2);

    useNotificationStore.getState().clearAll();

    expect(useNotificationStore.getState().notifications).toHaveLength(0);
  });
});
