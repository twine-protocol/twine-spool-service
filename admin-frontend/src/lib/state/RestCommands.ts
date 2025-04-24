export type ListCall<T> = () => Promise<T[]>;
export type CreateCall<T, U> = (data: U) => Promise<T>;
export type UpdateCall<T, U> = (id: string, data: U) => Promise<T>;
export type RemoveCall = (id: number|string) => Promise<void>;