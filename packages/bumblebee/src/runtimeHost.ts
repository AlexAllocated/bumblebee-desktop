/** Asset URLs only; provider access and authorization belong to the application. */
export type BumblebeeRuntimeHostOptions = { assetBaseUrl?: string | URL };
export type BumblebeeRuntimeHost = {
  readonly httpBaseUrl: URL;
  httpUrl: (path: string) => string;
};
export const createRuntimeHost = (
  options?: BumblebeeRuntimeHostOptions,
): BumblebeeRuntimeHost => {
  const base =
    options?.assetBaseUrl ??
    (typeof document !== "undefined"
      ? new URL(".", document.baseURI)
      : undefined);
  if (!base)
    throw new Error("Provide assetBaseUrl when rendering outside a document.");
  const httpBaseUrl = new URL(base);
  if (!httpBaseUrl.pathname.endsWith("/")) httpBaseUrl.pathname += "/";
  return {
    httpBaseUrl,
    httpUrl: (path) =>
      new URL(path.replace(/^\/+/, ""), httpBaseUrl).toString(),
  };
};
