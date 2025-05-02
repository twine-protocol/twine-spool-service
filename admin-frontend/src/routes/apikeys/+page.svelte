<script lang="ts">
  import { StructuredList, StructuredListHead, StructuredListBody, StructuredListRow, StructuredListSkeleton, StructuredListCell, Button, Tile, ComposedModal, ModalHeader, ModalBody, Form, ModalFooter, TextInput, DatePicker, DatePickerInput, Row, Column, Grid } from 'carbon-components-svelte'
  import * as ApiKeys from '$lib/state/ApiKeys'
	import { Add } from 'carbon-icons-svelte'
	import { onMount } from 'svelte'

  let createDialog = $state(false)

  let apiKeys : Promise<Array<ApiKeys.ApiKey>> = $state(Promise.resolve([]))
  let createdKey = $state("")

  const defaultData = () => ({
    description: '',
    expires_at: ''
  })

  let keyData = $state(defaultData())

  onMount(() => {
    apiKeys = ApiKeys.list()
  })

  const cancelCreate = () => {
    createDialog = false
    keyData = defaultData()
  }

  async function submit() {
    try {
      const bytes = new Uint8Array(32)
      crypto.getRandomValues(bytes)
      const key = Array.from(bytes, b => b.toString(16).padStart(2, '0')).join('')
      await ApiKeys.create({
        key,
        description: keyData.description,
        expires_at: keyData.expires_at
      })
      createdKey = key
      cancelCreate()
      apiKeys = ApiKeys.list()
    } catch (error) {
      console.error('Error creating API key:', error)
    }
  }


  async function remove(id: number){
    try {
      await ApiKeys.remove(id)
      cancelCreate()
      apiKeys = ApiKeys.list()
    } catch (error) {
      console.error('Error creating API key:', error)
    }
  }

</script>

{#if createdKey}
<Tile>
  <Grid>
  <Row>
    <Column style="align-content: center;">
      <p>API Key Created. Save it now. You won't be able to see it after navigating away.</p>
    </Column>
    <Column>
      <TextInput
        readonly
        value={createdKey}
      />
    </Column>
  </Row>
  </Grid>
</Tile>
{/if}

{#await apiKeys}
  <StructuredListSkeleton />
{:then apiKeys}
    <StructuredList>
      <StructuredListHead>
        <StructuredListRow head>
          <StructuredListCell head>Description</StructuredListCell>
          <StructuredListCell head>Created</StructuredListCell>
          <StructuredListCell head>Last Used</StructuredListCell>
          <StructuredListCell head>Expires</StructuredListCell>
          <StructuredListCell head>Actions</StructuredListCell>
        </StructuredListRow>
      </StructuredListHead>
      <StructuredListBody>
        {#each apiKeys as key}
        <StructuredListRow>
          <StructuredListCell>{key.description}</StructuredListCell>
          <StructuredListCell>{key.created_at}</StructuredListCell>
          <StructuredListCell>{key.last_used_at}</StructuredListCell>
          <StructuredListCell>{key.expires_at}</StructuredListCell>
          <StructuredListCell>
            <Button kind="danger" on:click={() => remove(key.id)}>Delete</Button>
          </StructuredListCell>
        </StructuredListRow>
        {/each}
      </StructuredListBody>
    </StructuredList>

{:catch error}
  <p>Error loading API keys: {error.message}</p>
{/await}

<Tile>
  <Button icon={Add} kind="primary" on:click={() => createDialog = true}>
    Create API Key
  </Button>
</Tile>

<ComposedModal bind:open={createDialog} on:submit={submit} on:close={cancelCreate} containerClass="modal-container-apikey">
  <ModalHeader label="Create API Key" />
  <ModalBody hasScrollingContent>
    <Form>
      <TextInput labelText="Description" bind:value={keyData.description} placeholder="Description" />
      <DatePicker datePickerType="single" dateFormat="Z" bind:value={keyData.expires_at}>
        <DatePickerInput labelText="Expiration Date" />
      </DatePicker>
    </Form>
  </ModalBody>
  <ModalFooter primaryButtonText="Create" secondaryButtonText="Cancel" />
</ComposedModal>

<style>
  :global(.modal-container-apikey .bx--modal-content) {
    min-height: 500px;
  }
</style>