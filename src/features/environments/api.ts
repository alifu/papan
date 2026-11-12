import { commands } from "../../ipc/bindings";
import type {
  EffectiveVariable,
  EnvironmentList,
  RequestLocation,
  SecretStorageStatus,
  TemplatePreview,
  VarScope,
  Variable,
  VariableInput,
} from "../../ipc/bindings";

// Thin wrappers over the generated bindings; unit tests `vi.mock` this module.
export const listEnvironments = (collectionId: string): Promise<EnvironmentList> =>
  commands.listEnvironments(collectionId);
export const createEnvironment = (collectionId: string, name: string): Promise<EnvironmentList> =>
  commands.createEnvironment(collectionId, name);
export const renameEnvironment = (collectionId: string, env: string, name: string): Promise<EnvironmentList> =>
  commands.renameEnvironment(collectionId, env, name);
export const deleteEnvironment = (collectionId: string, env: string): Promise<EnvironmentList> =>
  commands.deleteEnvironment(collectionId, env);
export const selectEnvironment = (collectionId: string, env: string | null): Promise<null> =>
  commands.selectEnvironment(collectionId, env);
export const readVariables = (scope: VarScope): Promise<Variable[]> => commands.readVariables(scope);
export const writeVariables = (scope: VarScope, variables: VariableInput[]): Promise<Variable[]> =>
  commands.writeVariables(scope, variables);
export const effectiveVariables = (location: RequestLocation): Promise<EffectiveVariable[]> =>
  commands.effectiveVariables(location);
export const previewTemplate = (location: RequestLocation, text: string): Promise<TemplatePreview> =>
  commands.previewTemplate(location, text);
export const secretStorageStatus = (): Promise<SecretStorageStatus> => commands.secretStorageStatus();
export const resetSecretStorage = (): Promise<SecretStorageStatus> => commands.resetSecretStorage();
