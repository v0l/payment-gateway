export interface Progress {
  required: number;
  received: number;
  confirmed: number;
  min_confirmations: number;
}

export interface Widget {
  provider: string;
  mode: string;
  token: string;
  email?: string;
  save_method?: boolean;
  checkout_url?: string;
}

export interface AttemptDetails {
  checkout_url?: string;
  widget?: Widget;
  bolt11?: string;
  address?: string;
  uri?: string;
  amount_sats?: number;
  deposit_address?: string;
  deposit_amount?: number | string;
  coin?: string;
}

export interface Attempt {
  id: NonNullable<Schemas['Attempt']['id']>;
  status: NonNullable<Schemas['Attempt']['status']>;
  amount: number;
  currency: string;
  details?: AttemptDetails;
  progress?: Progress;
}

export interface Method {
  provider_id: number;
  method: string;
  fee: number;
  currency: string;
  options: string[];
}

export interface FooterLink {
  label: string;
  url: string;
}

export interface Branding {
  default_lang?: string;
  theme?: string;
  radius?: string;
  font?: string;
  accent_dark?: string;
  logo_dark_url?: string;
  thanks_text?: string;
  footer_links?: FooterLink[];
  show_powered_by?: boolean;
}

import type { components } from '../api/schema';

type Schemas = components['schemas'];

export type LineItem = Schemas['LineItem'] & { name: string; quantity: number; total: number };

export interface PayState {
  id: string;
  status: string;
  amount: number;
  currency: string;
  description?: string;
  expires: string;
  server_time?: string;
  return_url?: string;
  receipt_url?: string;
  allowed_origins?: string[];
  can_save_card?: boolean;
  tenant: { name: string; logo_url?: string; accent_color?: string };
  branding?: Branding;
  attempts?: Attempt[];
  methods: Method[];
  line_items?: LineItem[];
  discount?: { amount: number; code?: string };
  tax?: number;
}
