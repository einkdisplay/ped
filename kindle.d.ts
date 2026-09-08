/**
 * Selects the requested FBInk waveform.
 *
 * "fast" uses WFM_DU and is intended for mostly black-and-white updates.
 * "quality" uses WFM_GC16 and is intended for grayscale content.
 * "auto" lets FBInk choose the waveform.
 */
type RefreshWaveform = "fast" | "quality" | "auto";

/**
 * Controls the waveform and flashing behavior of a refresh.
 *
 * A flashing refresh requests a full update intended to reduce accumulated
 * ghosting and artifacts. It should not be used for routine updates.
 */
type RefreshPolicy =
  | {
      /** Uses the quality waveform by default. */
      waveform?: "quality";

      /** Requests a full flashing refresh. */
      flashing?: boolean;
    }
  | {
      /** Uses WFM_DU or lets FBInk choose the waveform. */
      waveform: Exclude<RefreshWaveform, "quality">;

      /** Fast and automatic waveform selection do not support flashing. */
      flashing?: false;
    };

interface RefreshRegionOptions {
  /**
   * Elements whose bounding rectangles should be included in the refresh.
   *
   * The implementation reads getBoundingClientRect() when the refresh is
   * prepared and refreshes the union of all requested rectangles.
   */
  elements?: readonly Element[];

  /**
   * Additional CSS-pixel rectangles to include in the refresh.
   */
  rects?: readonly DOMRectInit[];
}

/**
 * Parameters for an immediate screen refresh.
 */
type RequestRefreshProps = RefreshPolicy & RefreshRegionOptions;

/**
 * A staged screen refresh.
 *
 * Adding elements or rectangles only records the requested regions. It does
 * not modify the framebuffer or refresh the physical screen until commit().
 */
interface KindleRefreshTransaction {
  /** Adds an element's bounding rectangle to the refresh union. */
  add(element: Element): void;

  /** Adds a CSS-pixel rectangle to the refresh union. */
  addRect(rect: DOMRectInit): void;

  /**
   * Renders and submits the union of all requested regions once.
   *
   * The Promise rejects if the refresh cannot be submitted successfully.
   */
  commit(): Promise<void>;

  /**
   * Cancels the staged refresh before commit().
   *
   * Calling abort() after commit() has no effect.
   */
  abort(): void;
}

interface KindleScreen {
  /**
   * The resolution of the Kindle screen.
   */
  readonly resolution: {
    /** The width of the Kindle screen, in pixels. */
    readonly width: number;
    /** The height of the Kindle screen, in pixels. */
    readonly height: number;
  };

  /** Whether automatic refresh is enabled. */
  readonly autoRefresh: boolean;

  /** The automatic refresh interval, in milliseconds. */
  readonly autoRefreshInterval: number;

  /**
   * Controls automatic refresh.
   *
   * @param enabled Whether to enable automatic refresh.
   * @param interval The interval in milliseconds. Defaults to 2500 ms.
   */
  setAutoRefresh(enabled: boolean, interval?: number): void;

  /**
   * Returns the time of the last successful physical screen refresh.
   *
   * Returns null if PED has not successfully refreshed the screen yet.
   */
  lastRefresh(): Date | null;

  /**
   * Refreshes the screen immediately.
   *
   * If elements or rectangles are supplied, their union is refreshed once.
   * If omitted, the full screen is refreshed.
   *
   * If props is omitted, the quality waveform is used without flashing.
   */
  refreshNow(props?: RequestRefreshProps): Promise<void>;

  /**
   * Starts a staged refresh transaction.
   *
   * Regions can be added with add() and addRect(). The framebuffer and
   * physical screen are not modified until commit().
   */
  beginRefresh(options?: RefreshPolicy): KindleRefreshTransaction;
}

interface KindleBatteryInfo {
  /**
   * The current battery percentage, ranging from 0 to 100.
   */
  readonly percentage: number;

  /** Whether the device is currently charging. */
  readonly charging: boolean;
}

interface KindleNetworkInfo {
  /** Whether the device is connected to the network. */
  readonly connected: boolean;

  /** SSID of the currently connected Wi-Fi network. */
  readonly ssid: string | null;

  /** Whether airplane mode is enabled. */
  readonly airplaneMode: boolean;

  /**
   * IPv4 address of the device, or null when it is not connected.
   * Loopback addresses are not returned.
   */
  readonly ipAddress: string | null;
}

interface KindleHardware {
  /**
   * Queries the current battery information.
   *
   * The Promise rejects if the device information cannot be acquired.
   */
  battery(): Promise<KindleBatteryInfo>;

  /**
   * Queries the current network information.
   *
   * The Promise rejects if the device information cannot be acquired.
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
