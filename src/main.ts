/*---------------------------------------------------------------------------------------------
 *  SideX — Tauri-based VSCode port
 *  Entry point. Globals set by inline script in index.html.
 *--------------------------------------------------------------------------------------------*/

async function boot() {
	// Import the web workbench barrel in stages so partial failures are isolated.
	// Each stage catches independently — a failure in one won't block the others.
	const stages = [
		['common',       () => import('./vs/workbench/workbench.common.main.js')],
		['web.main',     () => import('./vs/workbench/browser/web.main.js')],
		['web-dialog',   () => import('./vs/workbench/browser/parts/dialogs/dialog.web.contribution.js')],
		['web-services', () => import('./vs/workbench/workbench.web.main.js')],
	] as const;

	for (const [label, loader] of stages) {
		try {
			await loader();
		} catch (e) {
			console.warn(`[SideX] Barrel stage "${label}" failed (non-fatal):`, e);
		}
	}

	const { create } = await import('./vs/workbench/browser/web.factory.js');

	if (document.readyState === 'loading') {
		await new Promise<void>(r => window.addEventListener('DOMContentLoaded', () => r()));
	}

	create(document.body, {
		windowIndicator: {
			label: 'SideX',
			tooltip: 'SideX — Tauri Code Editor',
			command: undefined as any,
		},
		productConfiguration: {
			nameShort: 'SideX',
			nameLong: 'SideX',
			applicationName: 'sidex',
			dataFolderName: '.sidex',
			version: '0.1.0',
		} as any,
		settingsSyncOptions: {
			enabled: false,
		},
		additionalBuiltinExtensions: [],
		defaultLayout: {
			editors: [],
		},
	});

	console.log('[SideX] Workbench created successfully');
}

boot().catch((err) => {
	console.error('[SideX] Fatal:', err);
	document.body.innerHTML = `<div style="padding:40px;color:#ccc;font-family:system-ui">
		<h2>SideX failed to start</h2>
		<pre style="color:#f88;white-space:pre-wrap">${(err as Error)?.stack || err}</pre>
	</div>`;
});
