import {it,expect,vi} from 'vitest';
vi.mock('@tauri-apps/api/core',()=>({invoke:vi.fn(async()=>null)}));
import {invoke} from '@tauri-apps/api/core';
import {api} from '../src/api';
it('exposes only scoped epoch-bound history operations from production API',async()=>{
expect(api.workflow).not.toHaveProperty('history');expect(api.workflow).not.toHaveProperty('restoreVersion');
await api.workflow!.versionBrowser!.restore('b','c','v',3,'e');expect(invoke).toHaveBeenLastCalledWith('restore_chapter_version_scoped',{bookId:'b',chapterId:'c',versionId:'v',expectedRevision:3,expectedEpoch:'e'});
await api.workflow!.versionBrowser!.label('b','c','v','定稿',2,'e');expect(invoke).toHaveBeenLastCalledWith('set_chapter_version_label',{bookId:'b',chapterId:'c',versionId:'v',label:'定稿',expectedLabelRevision:2,expectedEpoch:'e'});
});
