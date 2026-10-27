/** Browser previews and other desktop platforms have no overlaid traffic lights. */
export function hasNativeMacTitlebar(native: boolean, platform: string): boolean {
  return native && /^Mac/.test(platform);
}
