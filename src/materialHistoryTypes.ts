import type {Material} from "./materialTypes";
export type MaterialContentVersion={id:string;material_id:string;material_revision:number;title:string;fields:Record<string,string>;created_at:number;source:string;byte_count:number};
export type MaterialVersionSummary=Pick<MaterialContentVersion,"id"|"material_revision"|"title"|"created_at"|"source">;
export type MaterialVersionPage={items:MaterialVersionSummary[];next_cursor:number|null;epoch:string};
export interface MaterialHistoryAPI{
 list(bookId:string,id:string,before:number|null,limit:number):Promise<MaterialVersionPage>;
 get(bookId:string,id:string,versionId:string):Promise<MaterialContentVersion>;
 restore(bookId:string,id:string,versionId:string,revision:number,epoch:string):Promise<Material>;
 acknowledge(bookId:string,id:string,revision:number,epoch:string):Promise<Material>;
}
