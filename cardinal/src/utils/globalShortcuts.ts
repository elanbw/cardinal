import { invoke } from '@tauri-apps/api/core';
import { register } from '@tauri-apps/plugin-global-shortcut';

// Option+Space, matching Everything's common Alt+Space binding on Windows.
export const QUICK_LAUNCH_SHORTCUT = 'Alt+Space';
export async function initializeGlobalShortcuts(): Promise<void> {
  try {
    await register(QUICK_LAUNCH_SHORTCUT, (event) => {
      // Fire on press so the window is ready sooner for keys typed right after the hotkey.
      if (event.state === 'Pressed') {
        void invoke('toggle_main_window');
      }
    });
  } catch (error) {
    console.error('Failed to register global shortcuts', error);
  }
}
