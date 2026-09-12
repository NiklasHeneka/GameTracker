import { call } from "./ipc";
import type { ConfigStatus } from "@/types/models";

export const configStatus = () => call<ConfigStatus>("config_status");
export const reloadConfig = () => call<ConfigStatus>("reload_config");
export const revealEnvFile = () => call<void>("reveal_env_file");
