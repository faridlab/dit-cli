import { describe, expect, it } from "vitest";
import { specIdFor } from "./RegisterSpecDialog";

describe("specIdFor", () => {
  it("names a spec after its folder when the file is called openapi or swagger", () => {
    expect(specIdFor("services/billing/openapi.yaml")).toBe("billing");
    expect(specIdFor("api/swagger.json")).toBe("api");
    expect(specIdFor("docs/payments-v2.yaml")).toBe("payments-v2");
    expect(specIdFor("2024/openapi.yml")).toBe("api-2024");
  });
});
