import type { ListCall, CreateCall, UpdateCall, RemoveCall } from './RestCommands'

export type DateString = string

export type ApiKey = {
  id: number
  description: string
  created_at: DateString
  last_used_at: DateString
  expires_at?: DateString
}

export const list: ListCall<ApiKey> = async () => {
  const response = await fetch('/api/apikeys', {
    method: 'GET',
    headers: {
      'Content-Type': 'application/json',
    },
  })
  if (!response.ok) {
    throw new Error('Failed to fetch API keys')
  }
  return response.json()
}

export const create: CreateCall<ApiKey, { description: string; expires_at?: DateString }> = async (data) => {
  const response = await fetch('/api/apikeys', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
    body: JSON.stringify(data),
  })
  if (!response.ok) {
    throw new Error('Failed to create API key')
  }
  return response.json()
}

export const remove: RemoveCall = async (id) => {
  const response = await fetch(`/api/apikeys/${id}`, {
    method: 'DELETE',
    headers: {
      'Content-Type': 'application/json',
    },
  })
  if (!response.ok) {
    throw new Error('Failed to delete API key')
  }
}
