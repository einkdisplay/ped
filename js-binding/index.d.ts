/**
 * Waveform type to use for a refresh.
 *
 * Waveforms here are modes, controlling how the E-ink screen will be updated.
 *
 * - `"fast"` is fastest, but may leave ghostings or artifacts on the screen.
 *   This is recommended when you need to do frequent updates, or for contents
 *   whose color is pure black or pure white. This uses WFM_DU under the hood.
 * - `"quality"` is slower (~200ms) than `"fast"` but less likely to produce
 *   ghostings and artifacts. Recommended for grayscale contents. This uses WFM_GC16
 *   under the hood.
 * - `"auto"` lets Kindle choose the waveform.
 */
type KindleRefreshWaveform = "fast" | "quality" | "auto";

/**
 * Flags for a refresh.
 */
interface KindleRefreshPolicy {
  /**
   * Waveform for the refresh.
   *
   * See {@link KindleRefreshWaveform} for details.
   *
   * @default "quality"
   */
  waveform?: KindleRefreshWaveform;

  /**
   * Whether to do a flashing during the refresh.
   *
   * A flashing (turning the whole screen black, and then white) may help E-ink
   * screens clean up ghostings and artifacts, but would be seriously visual-disruptive.
   * Use it sparingly.
   *
   * Note: PED may reject flashing with non-`quality` waveforms.
   *
   * @default false
   */
  flashing?: boolean;
}

/**
 * Options for specifying partial screen regions to refresh.
 */
interface KindleRefreshRegionOptions {
  /**
   * DOM elements whose bounding boxes should be refreshed.
   *
   * PED automatically measures their `getBoundingClientRect()` at refresh time
   * and triggers a single refresh covering the bounding union of all given elements.
   */
  elements?: readonly Element[];

  /**
   * Additional explicit bounding boxes (in CSS pixels) to include in the refresh area.
   */
  rects?: readonly DOMRectInit[];
}

/**
 * Configuration options for an immediate screen refresh (`KindleScreen.refreshNow()`).
 *
 * Combines refresh policies (waveform, flashing) with target regions (elements, rects).
 *
 * See: {@link KindleRefreshPolicy} and {@link KindleRefreshRegionOptions}
 */
type KindleRequestRefreshProps =
  & KindleRefreshPolicy
  & KindleRefreshRegionOptions;

/**
 * A staged refresh session for batching multiple region updates into a single screen redraw.
 *
 * Calls to `add()` or `addRect()` only queue regions into PED's own memory. Nothing is updated
 * on the physical E-ink display or the system framebuffer until you call `commit()`.
 */
interface KindleRefreshTransaction {
  /**
   * Queues an element's bounding rectangle to be refreshed in this transaction.
   */
  add(element: Element): void;

  /**
   * Queues a manual CSS-pixel rectangle to be refreshed in this transaction.
   */
  addRect(rect?: DOMRectInit): void;

  /**
   * Renders all queued regions to the screen in a single refresh operation.
   *
   * @returns A promise that resolves when the display update has been submitted.
   */
  commit(): Promise<void>;

  /**
   * Cancels the staged refresh and discards all queued regions.
   *
   * Has no effect if `commit()` has already been called.
   */
  abort(): void;
}

/**
 * Interface for interacting with the Kindle E-ink screen and controlling display updates.
 */
interface KindleScreen {
  /**
   * Screen width in device pixels.
   */
  readonly width: number;

  /**
   * Screen height in device pixels.
   */
  readonly height: number;

  /**
   * Whether auto-refresh is currently active.
   */
  readonly autoRefresh: boolean;

  /**
   * The current interval for auto-refresh in milliseconds.
   */
  readonly autoRefreshInterval: number;

  /**
   * Enable or disable periodic background screen refreshes.
   *
   * Useful for keeping UI fresh without manual calls, though frequent updates
   * will impact battery life.
   *
   * @param enabled Whether to enable automatic background refreshing.
   * @param interval How often to refresh, in milliseconds. Defaults to 2500ms.
   */
  setAutoRefresh(enabled: boolean, interval?: number): void;

  /**
   * Gets the timestamp (in Unix epoch milliseconds) of the most recent physical screen refresh.
   *
   * @returns Epoch milliseconds, or `null` if no physical refresh has occurred since system launch.
   */
  lastRefresh(): number | null;

  /**
   * Triggers an immediate screen refresh.
   *
   * If `elements` or `rects` are specified, only the union area covering those targets
   * will be updated. If omitted, the entire screen is refreshed.
   *
   * Defaults to using the `"quality"` waveform without flashing.
   */
  refreshNow(props?: KindleRequestRefreshProps): Promise<void>;

  /**
   * Begins a staged transaction for batching multiple refresh regions.
   *
   * Use this when you need to update multiple non-contiguous areas at once without
   * causing multiple jarring physical screen flashes.
   *
   * @param options Refresh policies (waveform, flashing) to apply when `commit()` is called.
   */
  beginRefresh(options?: KindleRefreshPolicy): KindleRefreshTransaction;
}

/**
 * Battery and power state details.
 */
interface KindleBatteryInfo {
  /**
   * Battery level as a percentage, ranging from `0` to `100`.
   */
  readonly percentage: number;

  /**
   * `true` if the device is currently plugged in and charging.
   */
  readonly charging: boolean;
}

/**
 * Network connectivity status and Wi-Fi details.
 */
interface KindleNetworkInfo {
  /**
   * Whether the device currently has an active network connection.
   */
  readonly connected: boolean;

  /**
   * SSID of the connected Wi-Fi network, or `null` if not connected to Wi-Fi.
   */
  readonly ssid: string | null;

  /**
   * Whether Airplane Mode is enabled.
   */
  readonly airplaneMode: boolean;

  /**
   * Assigned IPv4 address of the device, or `null` if disconnected.
   * Note: Loopback addresses (e.g., `127.0.0.1`) are ignored and returned as `null`.
   */
  readonly ipAddress: string | null;
}

/**
 * System hardware info and power/network diagnostics.
 */
interface KindleHardware {
  /**
   * Fetches current battery status and charge state.
   *
   * @throws Rejects if hardware battery telemetry cannot be read.
   */
  battery(): Promise<KindleBatteryInfo>;

  /**
   * Fetches current network connection and Wi-Fi information.
   *
   * @throws Rejects if network state cannot be queried.
   */
  network(): Promise<KindleNetworkInfo>;
}

interface Kindle {
  readonly screen: KindleScreen;
  readonly device: KindleHardware;
}

interface Navigator {
  readonly kindle: Kindle;
}
